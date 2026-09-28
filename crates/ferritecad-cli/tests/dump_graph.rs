// SPDX-License-Identifier: MIT
//! `dump-graph` through the real CLI: read a current document, refuse the rest.
#![allow(clippy::panic)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::SystemTime;

use rusqlite::{Connection, OpenFlags, params};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}

fn dump(path: &Path, format: Option<&str>) -> Output {
    let mut cmd = cli();
    cmd.arg("dump-graph").arg(path);
    if let Some(format) = format {
        cmd.args(["--format", format]);
    }
    cmd.output().expect("dump-graph process")
}

fn create_sample(path: &Path) {
    let output = cli()
        .args(["create", "--sample"])
        .arg(path)
        .output()
        .expect("create process");
    assert!(
        output.status.success(),
        "create --sample failed: {output:?}"
    );
}

/// Fixture preparation only. The command under test never uses this connection.
fn prepare(path: &Path, statements: &str) {
    let connection = Connection::open(path).expect("fixture writer");
    connection.execute_batch(statements).expect("fixture sql");
}

fn schema_version(path: &Path) -> i64 {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("read schema version");
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("user_version")
}

fn sentinel(path: &Path, suffix: &str, bytes: &[u8]) {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    std::fs::write(PathBuf::from(name), bytes).expect("private sentinel");
}

fn remove_sentinel(path: &Path, suffix: &str) {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    std::fs::remove_file(PathBuf::from(name)).expect("remove own sentinel");
}

/// File bytes and mtimes only. Directory mtime moves when the test itself
/// creates or removes a fixture, so it is not evidence about the command.
fn files(root: &Path) -> BTreeMap<OsString, (Vec<u8>, SystemTime)> {
    std::fs::read_dir(root)
        .expect("directory")
        .map(|entry| {
            let entry = entry.expect("entry");
            let metadata = entry.metadata().expect("metadata");
            assert!(
                metadata.is_file(),
                "unexpected directory entry: {}",
                entry.path().display()
            );
            (
                entry.file_name(),
                (
                    std::fs::read(entry.path()).expect("bytes"),
                    metadata.modified().expect("mtime"),
                ),
            )
        })
        .collect()
}

fn assert_stored(root: &Path, before: &BTreeMap<OsString, (Vec<u8>, SystemTime)>, context: &str) {
    let after = files(root);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "{context}: directory entries changed"
    );
    for (name, (bytes, modified)) in before {
        let (new_bytes, new_modified) = &after[name];
        if new_bytes != bytes {
            let at = bytes
                .iter()
                .zip(new_bytes.iter())
                .position(|(left, right)| left != right)
                .unwrap_or(bytes.len().min(new_bytes.len()));
            panic!(
                "{context}: {name:?} bytes changed at {at} (len {} -> {})",
                bytes.len(),
                new_bytes.len()
            );
        }
        assert_eq!(new_modified, modified, "{context}: {name:?} mtime changed");
    }
}

fn rendered(root: &Path, path: &Path) -> (Vec<u8>, Vec<u8>) {
    let before = files(root);
    let mut text = None;
    let mut dot = None;
    for format in [None, Some("text"), Some("dot")] {
        let output = dump(path, format);
        assert_eq!(output.status.code(), Some(0), "{format:?} {output:?}");
        assert!(output.stderr.is_empty(), "{format:?} {output:?}");
        assert!(!output.stdout.is_empty(), "{format:?}");
        match format {
            None => text = Some(output.stdout),
            Some("text") => {
                assert_eq!(
                    output.stdout.as_slice(),
                    text.as_deref().expect("default text")
                );
            }
            Some("dot") => dot = Some(output.stdout),
            Some(other) => panic!("unexpected format {other}"),
        }
        assert_stored(root, &before, "successful dump-graph");
    }
    let text = text.expect("default text");
    let dot = dot.expect("dot");
    assert_ne!(text, dot);
    assert!(
        text.windows(6).any(|window| window == b"needs "),
        "text graph lost dependency lines"
    );
    assert!(dot.starts_with(b"digraph features {\n"), "{dot:?}");
    assert!(dot.ends_with(b"}\n"), "{dot:?}");
    assert!(dot.windows(4).any(|window| window == b" -> "));
    (text, dot)
}

fn refused(root: &Path, path: &Path, kind: &str, reason: &str) {
    let before = files(root);
    let mut stderrs = Vec::new();
    for format in [None, Some("text"), Some("dot")] {
        let output = dump(path, format);
        let storage_changed = files(root) != before;
        if output.status.code() != Some(2) || !output.stdout.is_empty() || storage_changed {
            panic!(
                "dump-graph format={format:?} exit={:?} stdout_len={} storage_changed={storage_changed} user_version={} stderr={}",
                output.status.code(),
                output.stdout.len(),
                schema_version_note(path),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert_stored(root, &before, "refused dump-graph");
        stderrs.push(output.stderr);
    }
    assert!(stderrs.iter().all(|stderr| stderr == &stderrs[0]));
    let stderr = String::from_utf8(stderrs[0].clone()).expect("utf-8 stderr");
    assert!(stderr.starts_with(&format!("error [{kind}]:")), "{stderr}");
    assert!(stderr.contains(reason), "{stderr}");
}

fn schema_version_note(path: &Path) -> String {
    match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(connection) => connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .map_or_else(|error| error.to_string(), |version| version.to_string()),
        Err(error) => error.to_string(),
    }
}

#[test]
fn current_sample_text_and_dot_leave_the_directory_unchanged() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("plate.fcad");
    create_sample(&path);
    assert_eq!(schema_version(&path), 3);
    sentinel(&path, "-cache", b"foreign cache");
    sentinel(&path, "-other", b"private sentinel");
    let (first_text, first_dot) = rendered(root.path(), &path);
    let (second_text, second_dot) = rendered(root.path(), &path);
    assert_eq!(first_text, second_text);
    assert_eq!(first_dot, second_dot);
    assert_eq!(schema_version(&path), 3);
}

#[test]
fn legacy_schema_wal_and_reader_floor_are_refused() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("plate.fcad");
    create_sample(&path);
    let current = std::fs::read(&path).expect("current bytes");
    prepare(
        &path,
        "DROP TABLE imported_source_refs; DROP TABLE imported_sources; PRAGMA user_version = 2;",
    );
    assert_eq!(schema_version(&path), 2, "fixture must be exact schema v2");
    sentinel(&path, "-other", b"keep other");
    refused(root.path(), &path, "unsupported", "needs migration");
    assert_eq!(schema_version(&path), 2, "dump-graph migrated schema v2");

    prepare(&path, "PRAGMA journal_mode = WAL;");
    sentinel(&path, "-wal", b"sentinel wal");
    sentinel(&path, "-shm", b"sentinel shm");
    refused(root.path(), &path, "unsupported", "journalling");

    std::fs::write(&path, &current).expect("restore DELETE header");
    refused(root.path(), &path, "unsupported", "WAL sidecar");

    remove_sentinel(&path, "-wal");
    remove_sentinel(&path, "-shm");
    prepare(&path, "UPDATE meta SET minimum_reader_version = 999");
    refused(root.path(), &path, "unsupported", "needs a reader");
}

#[test]
fn missing_and_broken_paths_create_nothing() {
    let root = tempfile::tempdir().expect("directory");
    let missing = root.path().join("missing.fcad");
    refused(root.path(), &missing, "io", "opening");
    assert!(!missing.exists());
    assert!(files(root.path()).is_empty());

    let nested = root.path().join("no-such-dir").join("plate.fcad");
    refused(root.path(), &nested, "io", "opening");
    assert!(!nested.parent().expect("parent").exists());
    assert!(files(root.path()).is_empty());

    let broken = root.path().join("broken.fcad");
    std::fs::write(&broken, b"not a SQLite document").expect("broken fixture");
    sentinel(&broken, "-other", b"keep other");
    refused(root.path(), &broken, "io", "application");
}

#[test]
fn clap_rejects_json_and_keeps_text_or_dot_help() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("plate.fcad");
    create_sample(&path);
    sentinel(&path, "-other", b"private sentinel");
    let before = files(root.path());

    let json = cli()
        .arg("dump-graph")
        .arg(&path)
        .args(["--format", "json"])
        .output()
        .expect("process");
    assert_eq!(json.status.code(), Some(2), "{json:?}");
    assert!(json.stdout.is_empty(), "{json:?}");
    let stderr = String::from_utf8_lossy(&json.stderr);
    assert!(stderr.contains("invalid value"), "{stderr}");
    assert!(stderr.contains("json"), "{stderr}");
    assert_stored(root.path(), &before, "rejected --format json");

    let help = cli()
        .args(["dump-graph", "--help"])
        .output()
        .expect("process");
    assert_eq!(help.status.code(), Some(0), "{help:?}");
    assert!(help.stderr.is_empty(), "{help:?}");
    let stdout = String::from_utf8_lossy(&help.stdout);
    assert!(stdout.contains("without migration"), "{stdout}");
    assert!(stdout.contains("read-only"), "{stdout}");
    assert!(
        stdout.contains("- text: Indented evaluation order"),
        "{stdout}"
    );
    assert!(
        stdout.contains("- dot:  Graphviz DOT, for `dot -Tsvg`"),
        "{stdout}"
    );
    assert!(!stdout.contains("json"), "{stdout}");
    assert_stored(root.path(), &before, "dump-graph --help");

    let usage = cli().arg("dump-graph").output().expect("process");
    assert_eq!(usage.status.code(), Some(2), "{usage:?}");
    assert!(usage.stdout.is_empty(), "{usage:?}");
    assert!(
        String::from_utf8_lossy(&usage.stderr).contains("Usage:"),
        "{usage:?}"
    );
    assert_stored(root.path(), &before, "dump-graph usage");
}

#[test]
fn read_only_permissions_still_dump_when_the_file_can_be_read() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("plate.fcad");
    create_sample(&path);
    sentinel(&path, "-cache", b"foreign cache");
    sentinel(&path, "-other", b"private sentinel");
    let (text, dot) = rendered(root.path(), &path);

    let original = path.metadata().expect("metadata").permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&path, readonly).expect("read-only file");
    let _restore_file = ResetPermissions {
        path: path.clone(),
        permissions: original,
    };
    let denied = std::fs::OpenOptions::new().write(true).open(&path);
    assert!(
        denied.is_err(),
        "test requires enforced write denial; privileged chmod is not evidence"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let directory = root.path().metadata().expect("directory").permissions();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o500))
            .expect("read-only directory");
        let _restore_dir = ResetPermissions {
            path: root.path().to_path_buf(),
            permissions: directory,
        };
        let probe = root.path().join("write-probe");
        assert!(
            std::fs::write(&probe, b"must be denied").is_err(),
            "directory still accepts a new file"
        );
        assert!(!probe.exists());
        let (again_text, again_dot) = rendered(root.path(), &path);
        assert_eq!(again_text, text);
        assert_eq!(again_dot, dot);
    }
    #[cfg(not(unix))]
    {
        let (again_text, again_dot) = rendered(root.path(), &path);
        assert_eq!(again_text, text);
        assert_eq!(again_dot, dot);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let before = files(root.path());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
            .expect("unreadable file");
        assert!(std::fs::read(&path).is_err(), "actual read denial");
        let output = dump(&path, None);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.starts_with("error [io]:"), "{stderr}");
        assert!(stderr.contains("opening"), "{stderr}");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("restore readability for the snapshot");
        assert_stored(root.path(), &before, "unreadable dump-graph");
    }
}

fn object_id(last: u8) -> [u8; 16] {
    [
        0x01, 0x90, 0x00, 0x00, 0x00, 0x00, 0x70, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        last,
    ]
}

fn object_text(last: u8) -> String {
    format!("01900000-0000-7000-8000-{last:012x}")
}

fn create_empty(path: &Path) {
    let output = cli()
        .arg("create")
        .arg(path)
        .output()
        .expect("create process");
    assert!(output.status.success(), "create failed: {output:?}");
}

/// Replace a sample's objects with copied datum-plane rows. Foreign keys stay
/// on unless the case is an endpoint the product writer would have rejected.
fn replace_with_planes(
    path: &Path,
    rows: &[([u8; 16], Option<&str>, i64)],
    edges: &[([u8; 16], [u8; 16], &str)],
    foreign_keys: bool,
) {
    let connection = Connection::open(path).expect("fixture writer");
    connection
        .pragma_update(
            None,
            "foreign_keys",
            if foreign_keys { "ON" } else { "OFF" },
        )
        .expect("foreign keys");
    let template: (String, i64, Option<Vec<u8>>, Vec<u8>, Vec<u8>) = connection
        .query_row(
            "SELECT kind, schema_version, parent_id, payload, payload_hash
             FROM objects WHERE kind = 'datum.plane'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("datum plane template");
    connection
        .execute_batch("DELETE FROM topology_refs; DELETE FROM deps; DELETE FROM objects;")
        .expect("clear sample graph");
    for (id, name, ordinal) in rows {
        connection
            .execute(
                "INSERT INTO objects
                    (id, kind, schema_version, parent_id, ordinal, name, payload, payload_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id.as_slice(),
                    &template.0,
                    template.1,
                    &template.2,
                    ordinal,
                    name,
                    &template.3,
                    &template.4
                ],
            )
            .expect("insert object");
    }
    for (dependent, dependency, role) in edges {
        connection
            .execute(
                "INSERT INTO deps (dependent_id, dependency_id, role) VALUES (?1, ?2, ?3)",
                params![dependent.as_slice(), dependency.as_slice(), role],
            )
            .expect("insert dependency");
    }
}

fn assert_dump_bytes(root: &Path, path: &Path, text: &[u8], dot: &[u8]) {
    let before = files(root);
    let mut seen = None;
    for format in [None, Some("text")] {
        let output = dump(path, format);
        assert_eq!(output.status.code(), Some(0), "{format:?} {output:?}");
        assert!(output.stderr.is_empty(), "{format:?} {output:?}");
        assert_eq!(output.stdout, text, "{format:?}");
        seen = Some(output.stdout);
        assert_stored(root, &before, "text dump-graph");
    }
    assert_eq!(seen.expect("text"), text);
    let output = dump(path, Some("dot"));
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(output.stdout, dot);
    assert_stored(root, &before, "dot dump-graph");
}

fn assert_text_refusal(root: &Path, path: &Path, stderr: &str) {
    let before = files(root);
    for format in [None, Some("text")] {
        let output = dump(path, format);
        assert_eq!(output.status.code(), Some(2), "{format:?} {output:?}");
        assert!(
            output.stdout.is_empty(),
            "{format:?} printed {} stdout bytes before refusing",
            output.stdout.len()
        );
        assert_eq!(output.stderr, stderr.as_bytes(), "{format:?}");
        assert_stored(root, &before, "refused text graph");
    }
}

fn assert_dot_bytes(root: &Path, path: &Path, exit: i32, stdout: &[u8], stderr: &[u8]) {
    let before = files(root);
    let output = dump(path, Some("dot"));
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert_eq!(output.stdout, stdout);
    assert_eq!(output.stderr, stderr);
    assert_stored(root, &before, "dot dump-graph");
}

#[test]
fn text_keeps_evaluation_order_and_every_stored_need() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("graph.fcad");
    create_sample(&path);
    // Neither object read order (5,2,3,4,1,6) nor UUID order is topological:
    // 1 must wait for 3 and 5. Initially-ready 2 and 5 use the UUID tie-break.
    let ids = [5, 2, 3, 4, 1, 6].map(object_id);
    // Edges are inserted out of read order. Every stored role must survive.
    replace_with_planes(
        &path,
        &[
            (ids[0], Some("plane α"), 0),
            (ids[1], None, 1),
            (ids[2], Some("sketch line"), 2),
            (ids[3], Some("side wall"), 3),
            (ids[4], Some("boss 板"), 4),
            (ids[5], Some("tip body"), 5),
        ],
        &[
            (ids[4], ids[2], "profile"),
            (ids[5], ids[4], "predecessor"),
            (ids[2], ids[0], "plane"),
            (ids[3], ids[2], "profile"),
            (ids[4], ids[0], "plane"),
            (ids[4], ids[2], "predecessor"),
        ],
        true,
    );

    let text = [
        format!("{} datum.plane", object_text(2)),
        format!("{} plane α", object_text(5)),
        format!("{} sketch line", object_text(3)),
        format!("    needs {} plane α [plane]", object_text(5)),
        format!("{} boss 板", object_text(1)),
        format!("    needs {} sketch line [predecessor]", object_text(3)),
        format!("    needs {} sketch line [profile]", object_text(3)),
        format!("    needs {} plane α [plane]", object_text(5)),
        format!("{} side wall", object_text(4)),
        format!("    needs {} sketch line [profile]", object_text(3)),
        format!("{} tip body", object_text(6)),
        format!("    needs {} boss 板 [predecessor]", object_text(1)),
    ]
    .join("\n")
        + "\n";

    let mut dot = String::from(
        "digraph features {\n  rankdir=LR;\n  node [shape=box, fontname=\"sans-serif\"];\n",
    );
    for (last, name) in [
        (5, "plane α"),
        (2, "-"),
        (3, "sketch line"),
        (4, "side wall"),
        (1, "boss 板"),
        (6, "tip body"),
    ] {
        dot.push_str(&format!(
            "  \"{}\" [label=\"{name}\\ndatum.plane\"];\n",
            object_text(last)
        ));
    }
    for (dependency, dependent, role) in [
        (3, 1, "predecessor"),
        (3, 1, "profile"),
        (5, 1, "plane"),
        (5, 3, "plane"),
        (3, 4, "profile"),
        (1, 6, "predecessor"),
    ] {
        dot.push_str(&format!(
            "  \"{}\" -> \"{}\" [label=\"{role}\"];\n",
            object_text(dependency),
            object_text(dependent)
        ));
    }
    dot.push_str("}\n");

    assert_dump_bytes(root.path(), &path, text.as_bytes(), dot.as_bytes());
    assert_eq!(schema_version(&path), 3);
}

#[test]
fn empty_document_text_is_empty_and_dot_is_only_the_header() {
    let root = tempfile::tempdir().expect("directory");
    let path = root.path().join("empty.fcad");
    create_empty(&path);
    assert_eq!(schema_version(&path), 3);
    let dot =
        "digraph features {\n  rankdir=LR;\n  node [shape=box, fontname=\"sans-serif\"];\n}\n";
    assert_dump_bytes(root.path(), &path, b"", dot.as_bytes());
}

#[test]
fn cycle_missing_endpoint_and_unknown_role_print_nothing() {
    let cycle_root = tempfile::tempdir().expect("directory");
    let cycle = cycle_root.path().join("cycle.fcad");
    create_sample(&cycle);
    let left = object_id(1);
    let right = object_id(2);
    replace_with_planes(
        &cycle,
        &[
            (left, Some("cycle left"), 0),
            (right, Some("cycle right"), 1),
        ],
        &[(left, right, "plane"), (right, left, "plane")],
        true,
    );
    let cycle_error = format!(
        "error [input]: invalid input: the feature graph contains a cycle among: {}, {}\n",
        object_text(1),
        object_text(2)
    );
    assert_text_refusal(cycle_root.path(), &cycle, &cycle_error);
    // DOT never asks for evaluation order, so a cycle is still a drawing.
    assert_dot_bytes(
        cycle_root.path(),
        &cycle,
        0,
        format!(
            "digraph features {{\n  rankdir=LR;\n  node [shape=box, fontname=\"sans-serif\"];\n  \"{left}\" [label=\"cycle left\\ndatum.plane\"];\n  \"{right}\" [label=\"cycle right\\ndatum.plane\"];\n  \"{right}\" -> \"{left}\" [label=\"plane\"];\n  \"{left}\" -> \"{right}\" [label=\"plane\"];\n}}\n",
            left = object_text(1),
            right = object_text(2)
        )
        .as_bytes(),
        b"",
    );

    let missing_root = tempfile::tempdir().expect("directory");
    let missing = missing_root.path().join("missing.fcad");
    create_sample(&missing);
    replace_with_planes(
        &missing,
        &[(left, Some("has a hole"), 0)],
        &[(left, object_id(9), "plane")],
        false,
    );
    let missing_error = format!(
        "error [input]: invalid input: object {} depends on {}, which is not in the graph\n",
        object_text(1),
        object_text(9)
    );
    assert_text_refusal(missing_root.path(), &missing, &missing_error);
    assert_dot_bytes(
        missing_root.path(),
        &missing,
        0,
        format!(
            "digraph features {{\n  rankdir=LR;\n  node [shape=box, fontname=\"sans-serif\"];\n  \"{present}\" [label=\"has a hole\\ndatum.plane\"];\n  \"{absent}\" -> \"{present}\" [label=\"plane\"];\n}}\n",
            present = object_text(1),
            absent = object_text(9)
        )
        .as_bytes(),
        b"",
    );

    let role_root = tempfile::tempdir().expect("directory");
    let role = role_root.path().join("role.fcad");
    create_sample(&role);
    replace_with_planes(
        &role,
        &[
            (left, Some("role source"), 0),
            (right, Some("role target"), 1),
        ],
        &[(right, left, "not_a_role")],
        true,
    );
    let role_error = "error [input]: invalid input: unknown dependency role \"not_a_role\"\n";
    assert_text_refusal(role_root.path(), &role, role_error);
    // The role is parsed with the edge list, after DOT has already printed nodes.
    assert_dot_bytes(
        role_root.path(),
        &role,
        2,
        format!(
            "digraph features {{\n  rankdir=LR;\n  node [shape=box, fontname=\"sans-serif\"];\n  \"{source}\" [label=\"role source\\ndatum.plane\"];\n  \"{target}\" [label=\"role target\\ndatum.plane\"];\n",
            source = object_text(1),
            target = object_text(2)
        )
        .as_bytes(),
        role_error.as_bytes(),
    );
}

struct ResetPermissions {
    path: PathBuf,
    permissions: std::fs::Permissions,
}

impl Drop for ResetPermissions {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.path, self.permissions.clone());
    }
}
