// SPDX-License-Identifier: MIT
//! §27E: the saved angle of a §27D sector, changed by `edit-revolve-angle`
//! into a new copy and measured after the fact.
//!
//! Every expected volume is this file's own Pappus × θ/360, never the
//! production helper. Each copy is rebuilt cold, from a cache that holds the
//! previous angle's archive, and from its own; its end faces are measured on
//! their planes under the exact saved UUIDs; its STL is read independently;
//! and every SQL cell outside the allowlist is compared with the source.
use super::partial::{
    angle, axis_line, check_partial_mesh, doc_sketch, expected_capabilities, measure_partial,
    pappus, payload_version, request_v2,
};
use super::*;
use ferritecad_document::{CapSide, RevolveAngle};
use ferritecad_kernel::{CancelToken, ProgressSink};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const EDIT_ANGLE: &str = "edit-revolve-angle";

/// A part with a bore, stepped, at fractional sizes, shifted along Y.
pub(super) const STEPPED_SHIFTED: [[f64; 2]; 6] = [
    [4.25, 1.5],
    [10.75, 1.5],
    [10.75, 6.5],
    [7.5, 6.5],
    [7.5, 16.25],
    [4.25, 16.25],
];
/// Solid, closed on the axis along its last Line, below and above Y = 0.
pub(super) const CYLINDER_SHIFTED: [[f64; 2]; 4] =
    [[0., -2.5], [9.75, -2.5], [9.75, 12.25], [0., 12.25]];
/// Solid, closed on the axis, a cone off the origin.
pub(super) const CONE_SHIFTED: [[f64; 2]; 3] = [[0., 0.5], [8.5, 0.5], [0., 13.75]];

pub(super) fn angle_edit(
    source: &Path,
    feature: &str,
    version: &str,
    request: &Path,
    out: &Path,
) -> Command {
    let mut c = cli();
    c.arg(EDIT_ANGLE)
        .arg(source)
        .arg("--feature")
        .arg(feature)
        .arg("--expect-version")
        .arg(version)
        .arg("--request")
        .arg(request)
        .arg("-o")
        .arg(out)
        .arg("--json");
    c
}

pub(super) fn angle_reply(out: Output, code: i32) -> Value {
    assert_eq!(out.status.code(), Some(code), "{out:?}");
    assert_eq!(out.stdout.iter().filter(|&&b| b == b'\n').count(), 1);
    let v: Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["operation"], EDIT_ANGLE);
    assert_eq!(v["ok"], code == 0, "{v}");
    v
}

pub(super) fn write_angle(path: &Path, degrees: f64) {
    write(path, &json!({"request_version":1,"angle_deg":degrees}));
}

/// The Revolve discovery reports, and the version to pin.
pub(super) fn target(path: &Path) -> (String, String, Value) {
    let c = inspect(path);
    let [r] = c["revolves"].as_array().expect("revolves").as_slice() else {
        panic!("one Revolve")
    };
    (
        r["feature_id"].as_str().expect("feature").to_owned(),
        c["content_version"].as_str().expect("version").to_owned(),
        r.clone(),
    )
}

/// The §27E allowlist: in `objects`, only the selected Revolve row's
/// `payload` and `payload_hash`; in `meta`, only `modified_at`. Nothing is
/// added or removed, and every other cell of every table is equal.
fn check_angle_cells(source: &Path, copy: &Path, feature: &str, changes: bool) {
    let feature = feature.parse::<ObjectId>().expect("UUID");
    let id = format!("id=Blob({:?})", feature.to_bytes().to_vec());
    let (a, b) = (super::edit::cells(source), super::edit::cells(copy));
    assert_eq!(
        a.keys().collect::<Vec<_>>(),
        b.keys().collect::<Vec<_>>(),
        "tables"
    );
    for (table, old) in &a {
        let new = &b[table];
        assert_eq!(old.len(), new.len(), "rows of {table}");
        let allowed = |row: &Vec<String>, cell: &String| match table.as_str() {
            "objects" => {
                row.contains(&id)
                    && (cell.starts_with("payload=") || cell.starts_with("payload_hash="))
            }
            "meta" => cell.starts_with("modified_at="),
            _ => false,
        };
        let strip = |rows: &Vec<Vec<String>>| -> Vec<Vec<String>> {
            let mut kept: Vec<Vec<String>> = rows
                .iter()
                .map(|r| r.iter().filter(|c| !allowed(r, c)).cloned().collect())
                .collect();
            kept.sort();
            kept
        };
        assert_eq!(
            strip(old),
            strip(new),
            "{table}: a cell outside the allowlist"
        );
        if table == "objects" {
            let changed: Vec<_> = old.iter().filter(|r| !new.contains(r)).collect();
            if changes {
                assert_eq!(changed.len(), 1, "exactly the Revolve row changes");
                assert!(changed[0].contains(&id));
            } else {
                assert!(changed.is_empty(), "the same angle rewrites nothing");
            }
        }
    }
}

pub(super) fn revolve_payload(path: &Path) -> Vec<u8> {
    let sql =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("SQL");
    sql.query_row(
        "SELECT payload FROM objects WHERE kind='feature.revolve'",
        [],
        |r| r.get(0),
    )
    .expect("payload")
}

pub(super) fn refs(path: &Path) -> Vec<TopologyRef> {
    let d = Document::open_read_only(path).expect("open");
    let mut refs = d.topology_refs().expect("refs");
    d.close().expect("close");
    refs.sort_by_key(|r| r.id);
    refs
}

pub(super) fn default_stl(path: &Path, out: &Path) -> Vec<u8> {
    let r = cli()
        .arg("export-stl")
        .arg(path)
        .arg("-o")
        .arg(out)
        .arg("--json")
        .output()
        .expect("STL");
    assert!(r.status.success(), "{r:?}");
    std::fs::read(out).expect("STL bytes")
}

/// Each profile is created at 137.5° and edited through 180° both ways —
/// 137.5 → 220 → 90 — and back to where it started, 90 → 137.5. Every copy
/// is measured, rebuilt from the previous angle's cache (a Miss: the new
/// angle may not return the old archive) and then from its own (a Hit), and
/// compared cell by cell with the file it was made from.
#[test]
fn native_revolve_angle_edits_measure_caps_names_cache_sql_and_exports() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let mut artifact = 0;
    for (name, points) in [
        ("stepped", STEPPED_SHIFTED.to_vec()),
        ("cylinder", CYLINDER_SHIFTED.to_vec()),
        ("cone", CONE_SHIFTED.to_vec()),
    ] {
        let full = pappus(&points);
        let solid = axis_line(&points).is_some();
        let input = d.path().join(format!("{name}.json"));
        let source = d.path().join(format!("{name}-137.5.fcad"));
        request_v2(&input, &points, angle(137.5));
        reply(create(&input, &source).output().expect("create"), 0);
        let (feature, _, row) = target(&source);
        assert_eq!(row["angle_edit"]["available"], true, "{row}");
        assert_eq!(row["angle_edit"]["refusal"], Value::Null);
        assert_eq!(row["angle_edit"]["document_refusal"], Value::Null);
        assert_eq!(
            row["angle_edit"]["min_deg"],
            json!(RevolveAngle::MIN_DEGREES)
        );
        assert_eq!(
            row["angle_edit"]["max_deg"],
            json!(RevolveAngle::MAX_DEGREES)
        );
        let saved_refs = refs(&source);
        let first_payload = revolve_payload(&source);
        // Measured before anything exports it: an export fills the cache.
        measure_partial(&source, &points, 137.5, full, Some(&[CacheOutcome::Miss]));
        measure_partial(&source, &points, 137.5, full, Some(&[CacheOutcome::Hit]));
        let first_stl = default_stl(&source, &d.path().join(format!("{name}-source.stl")));

        let mut previous = source.clone();
        let mut seen = vec![137.5f64.to_bits()];
        let mut copies = Vec::new();
        for degrees in [220., 90., 137.5] {
            let (id, version, _) = target(&previous);
            assert_eq!(id, feature, "the same Revolve throughout");
            let request = d.path().join(format!("{name}-{degrees}.json"));
            write_angle(&request, degrees);
            let out = d
                .path()
                .join(format!("{name}-{}-to-{degrees}.fcad", copies.len()));
            let kept = std::fs::read(&previous).expect("source");
            let v = angle_reply(
                angle_edit(&previous, &feature, &version, &request, &out)
                    .output()
                    .expect("edit"),
                0,
            );
            assert_eq!(v["result"]["feature_id"], feature.as_str());
            assert_eq!(v["result"]["destination"], out.to_str().expect("UTF-8"));
            assert_eq!(
                v["result"]["document_id"],
                inspect(&source)["document_id"],
                "the same document"
            );
            assert_eq!(std::fs::read(&previous).expect("source"), kept);
            check_angle_cells(&previous, &out, &feature, true);
            assert_eq!(refs(&out), saved_refs, "every saved name, unchanged");
            assert_eq!(payload_version(&out), if solid { 4 } else { 3 });
            assert_eq!(capability_rows(&out), expected_capabilities(solid));

            let c = inspect(&out);
            let r = &c["revolves"][0];
            assert_eq!(r["angle_deg"], json!(degrees));
            assert_eq!(r["extent"], "partial_turn");
            assert_eq!(r["angle_edit"]["available"], true);
            // §27F: the sector's profile is editable, and reports the angle
            // this copy now has.
            let sketch = &c["sketches"][0];
            assert_eq!(sketch["editable"], true, "{sketch}");
            assert_eq!(sketch["profile_feature"]["angle_deg"], json!(degrees));
            assert_eq!(
                sketch["profile_feature"]["kind"],
                if solid {
                    "partial_turn_revolve_axis_closed"
                } else {
                    "partial_turn_revolve"
                }
            );

            // Every archive the chain has built so far, under this copy's
            // name. The key moves with the angle, so a new angle is a Miss and
            // never a stale Hit; back at 137.5° the source's own archive,
            // carried along the chain, is the one that answers.
            std::fs::copy(
                previous.with_extension("fcad-cache"),
                out.with_extension("fcad-cache"),
            )
            .expect("cache copy");
            let first = if seen.contains(&degrees.to_bits()) {
                CacheOutcome::Hit
            } else {
                CacheOutcome::Miss
            };
            seen.push(degrees.to_bits());
            let cold = measure_partial(&out, &points, degrees, full, None);
            assert_eq!(
                cold,
                measure_partial(&out, &points, degrees, full, Some(&[first]))
            );
            assert_eq!(
                cold,
                measure_partial(&out, &points, degrees, full, Some(&[CacheOutcome::Hit]))
            );
            let listed = cli()
                .arg("print-topology")
                .arg(&out)
                .output()
                .expect("print-topology");
            let listed = String::from_utf8(listed.stdout).expect("UTF-8");
            let named = saved_refs.len();
            assert!(
                listed.contains(&format!("{named} of {named} references resolved")),
                "{listed}"
            );
            let m = mesh(&out, &out.with_extension("stl"));
            check_partial_mesh(&m, &points, degrees, full);
            if degrees == 220. {
                if let Some(dir) = std::env::var_os("FCAD_REVOLVE_ANGLE_ARTIFACTS") {
                    let dir = Path::new(&dir);
                    std::fs::create_dir_all(dir).expect("artifacts");
                    // Both at the default tessellation, the only one
                    // export-fbx has, so the CI join compares one mesh.
                    for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
                        let target = dir.join(format!("revolve-angle-{artifact}.{extension}"));
                        let r = cli()
                            .arg(op)
                            .arg(&out)
                            .arg("-o")
                            .arg(&target)
                            .arg("--json")
                            .output()
                            .expect("artifact");
                        assert!(r.status.success(), "{r:?}");
                    }
                }
                artifact += 1;
            }
            copies.push(out.clone());
            previous = out;
        }
        // A → B → A: back at 137.5° the Revolve is stored exactly as it was
        // created, and the part it builds is the same mesh, byte for byte.
        assert_eq!(revolve_payload(&previous), first_payload);
        assert_eq!(
            default_stl(&previous, &d.path().join(format!("{name}-back.stl"))),
            first_stl
        );
        // The 90° copy's archive alone, handed to the copy back at 137.5°,
        // does not answer for it.
        let alone = d.path().join(format!("{name}-alone.fcad"));
        std::fs::copy(&previous, &alone).expect("copy");
        let ninety = d.path().join(format!("{name}-ninety.fcad"));
        std::fs::copy(&copies[1], &ninety).expect("copy");
        measure_partial(&ninety, &points, 90., full, Some(&[CacheOutcome::Miss]));
        std::fs::copy(
            ninety.with_extension("fcad-cache"),
            alone.with_extension("fcad-cache"),
        )
        .expect("cache copy");
        measure_partial(&alone, &points, 137.5, full, Some(&[CacheOutcome::Miss]));
        measure_partial(&alone, &points, 137.5, full, Some(&[CacheOutcome::Hit]));

        // The same angle is accepted and publishes a copy whose stored
        // Revolve is byte-identical: only the modification time may move.
        let (_, version, _) = target(&source);
        let request = d.path().join(format!("{name}-same.json"));
        write_angle(&request, 137.5);
        let same = d.path().join(format!("{name}-same.fcad"));
        angle_reply(
            angle_edit(&source, &feature, &version, &request, &same)
                .output()
                .expect("edit"),
            0,
        );
        check_angle_cells(&source, &same, &feature, false);
        assert_eq!(revolve_payload(&same), first_payload);
    }
    assert_eq!(artifact, 3);
    // Both ends of the accepted range, and a fraction with no finite
    // decimal, are stored exactly and built as sectors.
    let input = d.path().join("edge.json");
    let source = d.path().join("edge.fcad");
    request_v2(&input, &BUSHING, angle(90.));
    reply(create(&input, &source).output().expect("create"), 0);
    let (feature, version, _) = target(&source);
    for degrees in [
        RevolveAngle::MIN_DEGREES,
        RevolveAngle::MAX_DEGREES,
        100. / 3.,
    ] {
        let request = d.path().join(format!("edge-{degrees}.json"));
        write_angle(&request, degrees);
        let out = d.path().join(format!("edge-{degrees}.fcad"));
        angle_reply(
            angle_edit(&source, &feature, &version, &request, &out)
                .output()
                .expect("edit"),
            0,
        );
        assert_eq!(inspect(&out)["revolves"][0]["angle_deg"], json!(degrees));
        measure_partial(&out, &BUSHING, degrees, PI * (100. - 16.) * 15., None);
    }
}

/// A sector written without a kernel: a hollow profile turned through
/// `degrees`, with its Line faces and both cap names.
pub(super) fn stub_sector(path: &Path, degrees: f64) -> ObjectId {
    let (revolve, _) = write_revolve_document(path, &BUSHING);
    let sketch = doc_sketch(path);
    let mut doc = Document::open(path).expect("open");
    doc.write(|w| {
        w.put_object(
            revolve,
            None,
            2,
            Some("Revolve1"),
            &ObjectPayload::Revolve(Revolve {
                profile: sketch,
                axis: RevolveAxis::SketchY,
                extent: RevolveExtent::Partial {
                    degrees: RevolveAngle::new(degrees).expect("angle"),
                },
                operation: SolidOperation::NewBody,
                axis_segment: None,
            }),
        )?;
        for side in [CapSide::Start, CapSide::End] {
            w.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: revolve,
                producer_feature: revolve,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::RevolveCap { side },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            })?;
        }
        Ok(())
    })
    .expect("sector");
    doc.close().expect("close");
    revolve
}

/// Discovery and every request refusal the command can reach before a
/// kernel, in any build; then what a kernel decides, said per build.
#[test]
fn revolve_angle_discovery_and_request_refusals_preserve_every_file() {
    let d = tempfile::tempdir().expect("dir");
    let source = d.path().join("sector.fcad");
    let revolve = stub_sector(&source, 137.5);
    let (feature, version, row) = target(&source);
    assert_eq!(feature, revolve.to_string());
    assert_eq!(
        row["angle_edit"],
        json!({"available":true,"refusal":null,"document_refusal":null,
               "min_deg":0.01,"max_deg":359.99})
    );
    // A full turn is reported, with its reason, and is not an angle.
    let full = d.path().join("full.fcad");
    write_revolve_document(&full, &BUSHING);
    let (_, _, full_row) = target(&full);
    assert_eq!(full_row["angle_edit"]["available"], false);
    assert!(
        full_row["angle_edit"]["refusal"]
            .as_str()
            .expect("refusal")
            .contains("full-turn Revolve has no angle"),
        "{full_row}"
    );
    assert_eq!(full_row["angle_deg"], Value::Null);

    let bytes = std::fs::read(&source).expect("source");
    let request = d.path().join("request.json");
    let out = d.path().join("never.fcad");
    let refuse = |body: &[u8], kind: &str, why: &str| -> String {
        std::fs::write(&request, body).expect("request");
        let names = entries(d.path());
        let v = angle_reply(
            angle_edit(&source, &feature, &version, &request, &out)
                .output()
                .expect("edit"),
            2,
        );
        assert_eq!(v["error"]["kind"], kind, "{why}: {v}");
        assert_eq!(entries(d.path()), names, "{why}: nothing written");
        assert_eq!(std::fs::read(&source).expect("source"), bytes, "{why}");
        v["error"]["message"].as_str().expect("message").to_owned()
    };
    for (body, why) in [
        (&b"{"[..], "malformed JSON"),
        (b"", "empty"),
        (b"[1,90]", "an array"),
        (
            br#"{"request_version":2,"request_version":1,"angle_deg":90}"#,
            "a duplicate version",
        ),
        (
            br#"{"request_version":1,"angle_deg":90,"angle_deg":220}"#,
            "a duplicate angle",
        ),
        (
            br#"{"request_version":1,"angle_deg":90,"angle_\u0064eg":220}"#,
            "an escaped duplicate angle",
        ),
        (br#"{"request_version":1}"#, "no angle"),
        (br#"{"angle_deg":90}"#, "no version"),
        (br#"{"request_version":1,"angle_deg":"90"}"#, "a string"),
        (br#"{"request_version":1,"angle_deg":null}"#, "null"),
        (br#"{"request_version":1,"angle_deg":[90]}"#, "a list"),
        (
            br#"{"request_version":1,"angle_deg":{"degrees":90}}"#,
            "an object",
        ),
        (
            br#"{"request_version":1,"angle_deg":{"degrees":90,"direction":"cw"}}"#,
            "an unknown nested field",
        ),
        (
            br#"{"request_version":1,"angle_deg":90,"direction":"cw"}"#,
            "an unknown field",
        ),
        (
            br#"{"request_version":1,"angle_deg":90,"extent":{"kind":"full_turn"}}"#,
            "an extent",
        ),
        (br#"{"request_version":1,"angle_deg":1e400}"#, "overflow"),
        (
            br#"{"request_version":"1","angle_deg":90}"#,
            "a string version",
        ),
    ] {
        refuse(body, "input", why);
    }
    let message = refuse(
        br#"{"request_version":2,"angle_deg":90}"#,
        "unsupported",
        "v2",
    );
    assert!(message.contains("expected 1"), "{message}");
    let mut large = br#"{"request_version":1,"angle_deg":90}"#.to_vec();
    large.resize(65537, b' ');
    let message = refuse(&large, "input", "oversize");
    assert!(message.contains("65536"), "{message}");
    // A path that is not UTF-8 cannot be reported in JSON v1.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        write_angle(&request, 90.);
        let names = entries(d.path());
        let bad = d.path().join(std::ffi::OsStr::from_bytes(b"out-\xff.fcad"));
        let v = angle_reply(
            angle_edit(&source, &feature, &version, &request, &bad)
                .output()
                .expect("edit"),
            2,
        );
        assert!(
            v["error"]["message"]
                .as_str()
                .expect("message")
                .contains("UTF-8"),
            "{v}"
        );
        assert_eq!(entries(d.path()), names);
    }
    // Usage stays clap's: no JSON, exit 2, nothing written.
    let names = entries(d.path());
    let usage = cli()
        .arg(EDIT_ANGLE)
        .arg(&source)
        .arg("--expect-version")
        .arg(&version)
        .arg("--request")
        .arg(&request)
        .arg("-o")
        .arg(&out)
        .arg("--json")
        .output()
        .expect("usage");
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
    assert_eq!(entries(d.path()), names);

    // The angle itself is the domain's to judge, inside the job, which a
    // build without a kernel never reaches: there every well-formed request
    // is refused for the missing kernel, whatever its angle.
    let native = ferritecad_occt::is_available();
    for (degrees, message) in [
        (0., "not positive"),
        (-90., "not positive"),
        (0.005, "below the 0.01° minimum"),
        (359.995, "within"),
        (360., "state a full turn"),
        (400., "more than one turn"),
        (137.5 + 360., "more than one turn"),
    ] {
        let body =
            serde_json::to_vec(&json!({"request_version":1,"angle_deg":degrees})).expect("JSON");
        if native {
            let text = refuse(&body, "input", message);
            assert!(text.contains(message), "{degrees}: {text}");
        } else {
            refuse(&body, "unsupported", message);
        }
    }
    if !native {
        let text = refuse(
            br#"{"request_version":1,"angle_deg":220}"#,
            "unsupported",
            "a valid angle without a kernel",
        );
        assert!(!text.is_empty());
    }
}

/// Everything a native job refuses, from real processes, with every file
/// left as it was.
#[test]
fn native_revolve_angle_edit_refusals_preserve_every_file() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("sector.json");
    let source = d.path().join("sector.fcad");
    request_v2(&input, &BUSHING, angle(90.));
    reply(create(&input, &source).output().expect("create"), 0);
    let (feature, version, _) = target(&source);
    let c = inspect(&source);
    let angle_request = d.path().join("angle.json");
    write_angle(&angle_request, 220.);
    let out = d.path().join("never.fcad");
    let refuse = |source: &Path, feature: &str, version: &str, out: &Path, why: &str| {
        let names = entries(d.path());
        let bytes = std::fs::read(source).expect("source");
        let v = angle_reply(
            angle_edit(source, feature, version, &angle_request, out)
                .output()
                .expect("edit"),
            2,
        );
        assert_eq!(std::fs::read(source).expect("source"), bytes, "{why}");
        assert_eq!(entries(d.path()), names, "{why}: nothing written");
        (
            v["error"]["kind"].as_str().expect("kind").to_owned(),
            v["error"]["message"].as_str().expect("message").to_owned(),
        )
    };
    // Identities: unknown, and objects that are not a Revolve.
    let (kind, message) = refuse(
        &source,
        &ObjectId::new().to_string(),
        &version,
        &out,
        "unknown",
    );
    assert_eq!(kind, "input");
    assert!(message.contains("does not exist"), "{message}");
    for id in [
        c["sketches"][0]["sketch_id"].as_str().expect("sketch"),
        c["bodies"][0]["body_id"].as_str().expect("body"),
    ] {
        let (kind, message) = refuse(&source, id, &version, &out, "not a Revolve");
        assert_eq!(kind, "unsupported");
        assert!(message.contains("not a Revolve"), "{message}");
    }
    // A full turn has no angle; it does not become a sector.
    let full_input = d.path().join("full.json");
    let full = d.path().join("full.fcad");
    request(&full_input, &BUSHING);
    reply(create(&full_input, &full).output().expect("create"), 0);
    let (full_feature, full_version, _) = target(&full);
    let (kind, message) = refuse(&full, &full_feature, &full_version, &out, "full turn");
    assert_eq!(kind, "unsupported");
    assert!(
        message.contains("full-turn Revolve has no angle"),
        "{message}"
    );
    // A stale version, and destinations that are the source or taken.
    let stale = ferritecad_types::ContentHash::of_bytes(b"another document").to_string();
    let (_, message) = refuse(&source, &feature, &stale, &out, "stale");
    assert!(message.contains("source has changed"), "{message}");
    let taken = d.path().join("taken.fcad");
    std::fs::write(&taken, b"keep").expect("taken");
    let (_, message) = refuse(&source, &feature, &version, &taken, "occupied");
    assert!(message.contains("already exists"), "{message}");
    assert_eq!(std::fs::read(&taken).expect("taken"), b"keep");
    let (_, message) = refuse(&source, &feature, &version, &source, "the source itself");
    assert!(message.contains("different files"), "{message}");
    #[cfg(unix)]
    {
        let link = d.path().join("link.fcad");
        std::os::unix::fs::symlink(&source, &link).expect("symlink");
        let hard = d.path().join("hard.fcad");
        std::fs::hard_link(&source, &hard).expect("hard link");
        for (alias, why) in [(&link, "symlink"), (&hard, "hard link")] {
            let (_, message) = refuse(&source, &feature, &version, alias, why);
            assert!(
                message.contains("different files") || message.contains("already exists"),
                "{why}: {message}"
            );
        }
        // A UTF-8 source path that is not ASCII is an ordinary path.
        let unicode = d.path().join("сектор-θ.fcad");
        std::fs::copy(&source, &unicode).expect("copy");
        let published = d.path().join("копия-θ.fcad");
        let v = angle_reply(
            angle_edit(&unicode, &feature, &version, &angle_request, &published)
                .output()
                .expect("edit"),
            0,
        );
        assert_eq!(
            v["result"]["destination"],
            published.to_str().expect("UTF-8")
        );
        assert_eq!(inspect(&published)["revolves"][0]["angle_deg"], 220.);
    }
    // Documents outside the class refuse at the job, by their structure.
    let variant = |name: &str, change: &dyn Fn(&mut Document)| -> PathBuf {
        let path = d.path().join(format!("{name}.fcad"));
        std::fs::copy(&source, &path).expect("copy");
        let mut doc = Document::open(&path).expect("open");
        change(&mut doc);
        doc.close().expect("close");
        path
    };
    let moved = variant("moved-plane", &|doc| {
        let plane = doc
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::DatumPlane(_)))
            .expect("plane");
        doc.write(|w| {
            w.put_object(
                plane.id,
                None,
                plane.ordinal,
                plane.name.as_deref(),
                &ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::from_translation(
                        ferritecad_types::Vec3::new(0., 0., 5.).expect("vector"),
                    )
                    .expect("transform"),
                }),
            )
        })
        .expect("move");
    });
    let axis_contact = variant("axis-contact", &|doc| {
        let sketch = doc
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("sketch");
        let ObjectPayload::Sketch(mut s) = sketch.payload.clone() else {
            panic!("a Sketch")
        };
        let points = [[0., 0.], [10., 0.], [10., 15.], [0., 15.]];
        for (i, curve) in s.curves.iter_mut().enumerate() {
            let (a, b) = (points[i], points[(i + 1) % 4]);
            curve.geometry = SketchGeometry::Line {
                start: Point2::new(a[0], a[1]).expect("point"),
                end: Point2::new(b[0], b[1]).expect("point"),
            };
        }
        doc.write(|w| {
            w.put_object(
                sketch.id,
                None,
                sketch.ordinal,
                sketch.name.as_deref(),
                &ObjectPayload::Sketch(s),
            )
        })
        .expect("rewrite");
    });
    // A saved name nothing resolves: the baseline rebuild refuses the copy.
    let lost_name = variant("lost-name", &|doc| {
        let revolve = feature.parse::<ObjectId>().expect("UUID");
        doc.write(|w| {
            w.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: revolve,
                producer_feature: revolve,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::RevolveFace {
                    profile_segment: StableEntityId::new(),
                },
                selection: SelectionRule::AllDerivedFrom {
                    ancestor: StableEntityId::new(),
                },
                fallback_signature: None,
            })
        })
        .expect("dangling name");
    });
    for (path, why, wanted) in [
        (moved, "moved plane", "untransformed XY plane"),
        (axis_contact, "profile class", "between hollow and solid"),
        (
            lost_name,
            "unresolved name",
            "unresolved topology references",
        ),
    ] {
        let (_, version, row) = target(&path);
        if why != "unresolved name" {
            assert_eq!(row["angle_edit"]["available"], false, "{why}");
        }
        let (_, message) = refuse(&path, &feature, &version, &out, why);
        assert!(message.contains(wanted), "{why}: {message}");
    }
}

/// The shared job at its own phases, with the kernel that ships, and a
/// report that cannot be delivered after publication.
#[test]
fn native_revolve_angle_edit_delivery_late_guards_and_cancellation() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("cone.json");
    let source = d.path().join("cone.fcad");
    request_v2(&input, &CONE_SHIFTED, angle(137.5));
    reply(create(&input, &source).output().expect("create"), 0);
    let full = pappus(&CONE_SHIFTED);
    let (feature, version, _) = target(&source);
    let request_path = d.path().join("angle.json");
    write_angle(&request_path, 220.);

    // Exit 7: the report is lost, the publication stands.
    for both in [false, true] {
        let published = d.path().join(format!("pipe-{both}.fcad"));
        let mut c = angle_edit(&source, &feature, &version, &request_path, &published);
        c.stdout(pipe::closed_pipe());
        if both {
            c.stderr(pipe::closed_pipe());
        } else {
            c.stderr(std::process::Stdio::piped());
        }
        let r = c.output().expect("closed stdout");
        assert_eq!(r.status.code(), Some(7), "{r:?}");
        measure_partial(&published, &CONE_SHIFTED, 220., full, None);
    }

    let reading = ferritecad_jobs::read_extrude_source(&source).expect("reading");
    let [choice] = reading.revolve_angles.as_slice() else {
        panic!("one Revolve")
    };
    assert_eq!(choice.refusal, None);
    let job = |name: &str| ferritecad_jobs::EditRevolveAngleRequest {
        source: source.clone(),
        expected: reading.version,
        feature: choice.feature,
        degrees: 220.,
        destination: d.path().join(name),
    };
    // A kernel that cannot build the saved part refuses at the baseline
    // rebuild, before anything is written or published.
    let names = entries(d.path());
    let error = ferritecad_jobs::edit_revolve_angle_copy(
        &job("mock.fcad"),
        &mut ferritecad_kernel::mock::MockKernel::new(),
        &OperationContext::default(),
    )
    .expect_err("no rebuild");
    assert!(
        error.to_string().contains("cannot turn a profile"),
        "{error}"
    );
    assert_eq!(entries(d.path()), names, "no copy and no scratch");
    // Cancelled after the baseline check, before the write: nothing at all.
    let token = CancelToken::new();
    let stop = token.clone();
    let context = OperationContext::default()
        .with_cancel(token)
        .with_progress(ProgressSink::new(move |f| {
            if f >= 0.4 {
                stop.cancel();
            }
        }));
    let error = ferritecad_jobs::edit_revolve_angle_copy(
        &job("cancelled.fcad"),
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &context,
    )
    .expect_err("cancelled");
    assert_eq!(error.kind(), ErrorKind::Cancellation, "{error}");
    assert_eq!(entries(d.path()), names, "no copy and no scratch");
    // Publication race: another writer takes the destination while the job
    // runs. Publication never replaces it.
    let raced = d.path().join("raced.fcad");
    let path = raced.clone();
    let context = OperationContext::default().with_progress(ProgressSink::new(move |f| {
        if f >= 0.95 && !path.exists() {
            std::fs::write(&path, b"theirs").expect("race");
        }
    }));
    let error = ferritecad_jobs::edit_revolve_angle_copy(
        &job("raced.fcad"),
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &context,
    )
    .expect_err("taken during the job");
    assert!(error.to_string().contains("exists"), "{error}");
    assert_eq!(std::fs::read(&raced).expect("theirs"), b"theirs");
    std::fs::remove_file(&raced).expect("clean up");
    assert_eq!(entries(d.path()), names, "no scratch left behind");
    // The source replaced while the job ran: the late guard refuses it.
    let bytes = std::fs::read(&source).expect("source");
    let late = d.path().join("late.fcad");
    let path = source.clone();
    let done = Arc::new(AtomicBool::new(false));
    let once = done.clone();
    let context = OperationContext::default().with_progress(ProgressSink::new(move |f| {
        if f >= 0.95 && !once.swap(true, Ordering::SeqCst) {
            let mut d = Document::open(&path).expect("source");
            let plane = d
                .objects()
                .expect("objects")
                .into_iter()
                .find(|o| matches!(o.payload, ObjectPayload::DatumPlane(_)))
                .expect("plane");
            d.write(|w| {
                w.put_object(
                    plane.id,
                    None,
                    plane.ordinal,
                    Some("renamed while editing"),
                    &plane.payload,
                )
            })
            .expect("change");
            d.close().expect("close");
        }
    }));
    let error = ferritecad_jobs::edit_revolve_angle_copy(
        &job("late.fcad"),
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &context,
    )
    .expect_err("stale at publication");
    assert!(done.load(Ordering::SeqCst), "the late phase ran");
    assert!(error.to_string().contains("source has changed"), "{error}");
    assert!(!late.exists());
    assert!(
        !entries(d.path())
            .iter()
            .any(|n| n.to_string_lossy().starts_with(".ferritecad-")),
        "no scratch left behind"
    );
    assert_ne!(std::fs::read(&source).expect("source"), bytes);
    // The prepared value of the old version is refused by the writer's own
    // gate when handed to the changed document directly.
    {
        let old = source.with_extension("old.fcad");
        std::fs::write(&old, &bytes).expect("old copy");
        let before = Document::open_read_only(&old).expect("old");
        let prepared = ferritecad_document::prepare_revolve_angle(&before, choice.feature, 220.)
            .expect("prepared");
        before.close().expect("close");
        let mut changed = Document::open(&source).expect("changed");
        let error = changed
            .write_revolve_angle(&prepared)
            .expect_err("prepared against another version");
        assert!(error.to_string().contains("changed after"), "{error}");
        changed.close().expect("close");
        std::fs::remove_file(&old).expect("clean up");
    }
    // Cancellation that arrives after publication does not undo it.
    let reading = ferritecad_jobs::read_extrude_source(&source).expect("reading");
    let token = CancelToken::new();
    let stop = token.clone();
    let context = OperationContext::default()
        .with_cancel(token)
        .with_progress(ProgressSink::new(move |f| {
            if f >= 1.0 {
                stop.cancel();
            }
        }));
    let mut request = job("after.fcad");
    request.expected = reading.version;
    let edited = ferritecad_jobs::edit_revolve_angle_copy(
        &request,
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &context,
    )
    .expect("published before the cancellation");
    assert_eq!(edited.feature, choice.feature);
    measure_partial(&request.destination, &CONE_SHIFTED, 220., full, None);
}

#[test]
fn occt_without_solver_edits_a_partial_revolve_angle() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("cylinder.json");
    let source = d.path().join("cylinder.fcad");
    request_v2(&input, &CYLINDER_SHIFTED, angle(137.5));
    reply(create(&input, &source).output().expect("create"), 0);
    let (feature, version, _) = target(&source);
    let request = d.path().join("angle.json");
    write_angle(&request, 220.);
    let out = d.path().join("edited.fcad");
    angle_reply(
        angle_edit(&source, &feature, &version, &request, &out)
            .output()
            .expect("edit"),
        0,
    );
    check_angle_cells(&source, &out, &feature, true);
    let full = pappus(&CYLINDER_SHIFTED);
    measure_partial(&out, &CYLINDER_SHIFTED, 220., full, None);
    let m = mesh(&out, &out.with_extension("stl"));
    check_partial_mesh(&m, &CYLINDER_SHIFTED, 220., full);
}
