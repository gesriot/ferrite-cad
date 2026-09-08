// SPDX-License-Identifier: MIT
//! Body discovery and STL process checks, sharing this suite's JSON/pipe readers.
use super::*;
use ferritecad_document::{Body, StepImportRequest};
use ferritecad_jobs::stl_bodies;

const TWIN: &str = "Twin \"Я\"\nline\\two";

pub(super) fn export(source: &Path, destination: &Path, body: Option<&str>, json: bool) -> Command {
    let mut command = cli();
    command
        .arg("export-stl")
        .arg(source)
        .arg("-o")
        .arg(destination);
    if let Some(body) = body {
        command.args(["--solid", body]);
    }
    if json {
        command.arg("--json");
    }
    command
}

fn twins(path: &Path) -> [ObjectId; 2] {
    let created = reply(
        &run(&mut create_json(path, Some(["60", "40", "10"]))),
        "create",
        0,
    );
    let reading = inspect(path);
    assert_eq!(reading["document_id"], created["result"]["document_id"]);
    let first = reading["bodies"][0]["body_id"]
        .as_str()
        .expect("discovery")
        .parse()
        .expect("UUIDv7");
    let feature: ObjectId = selected(&reading, "Extrude1")
        .parse()
        .expect("Extrude UUID");
    assert_ne!(first, feature);
    let mut document = Document::open(path).expect("fixture");
    let body = document.object(first).expect("Body").expect("stored");
    assert!(matches!(body.payload, ObjectPayload::Body(_)));
    let ObjectPayload::Extrude(mut extrude) = document
        .object(feature)
        .expect("feature")
        .expect("stored")
        .payload
    else {
        panic!("feature domain");
    };
    extrude.end_condition = EndCondition::Blind {
        distance: Expression::constant(23.0).expect("height"),
    };
    let second = ObjectId::new();
    let second_feature = ObjectId::new();
    document
        .write(|w| {
            w.put_object(first, body.parent, body.ordinal, Some(TWIN), &body.payload)?;
            w.put_object(
                second_feature,
                None,
                4,
                None,
                &ObjectPayload::Extrude(extrude.clone()),
            )?;
            w.add_dependency(Dependency {
                dependent: second_feature,
                dependency: extrude.profile,
                role: DependencyRole::Profile,
            })?;
            w.put_object(
                second,
                None,
                5,
                Some(TWIN),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(second_feature),
                }),
            )?;
            w.add_dependency(Dependency {
                dependent: second,
                dependency: second_feature,
                role: DependencyRole::BodyTip,
            })?;
            Ok(())
        })
        .expect("two different bodies");
    document.close().expect("close");
    [first, second]
}

fn rename(path: &Path, id: ObjectId, name: Option<&str>, ordinal: i64) {
    let mut document = Document::open(path).expect("fixture");
    let object = document.object(id).expect("read").expect("object");
    document
        .write(|w| w.put_object(id, object.parent, ordinal, name, &object.payload))
        .expect("rename/reorder");
    document.close().expect("close");
}

fn body_ids(reading: &Value) -> Vec<ObjectId> {
    reading["bodies"]
        .as_array()
        .expect("required array")
        .iter()
        .map(|body| {
            body["body_id"]
                .as_str()
                .expect("string")
                .parse()
                .expect("canonical UUIDv7")
        })
        .collect()
}

fn files(root: &Path) -> Vec<(OsString, Vec<u8>)> {
    entries(root)
        .into_iter()
        .map(|name| {
            let bytes = std::fs::read(root.join(&name)).expect("file bytes");
            (name, bytes)
        })
        .collect()
}

fn refusal(command: &mut Command, kind: &str, root: &Path) {
    let before = files(root);
    let value = reply(&run(command), "export-stl", 2);
    assert_eq!(value["error"]["kind"], kind, "{value}");
    assert_eq!(
        files(root),
        before,
        "no changed source/output or leftover scratch"
    );
}

#[test]
fn body_discovery_preserves_domains_order_nulls_and_the_pinned_reading() {
    let root = tempfile::tempdir().expect("dir");
    let source = root.path().join("Каталог с пробелами.fcad");
    let [first, second] = twins(&source);
    let initial = inspect(&source);
    assert_eq!(body_ids(&initial), [first, second]);
    assert_eq!(initial["bodies"][0]["name"], TWIN);
    assert_eq!(initial["bodies"][1]["name"], TWIN);
    rename(&source, second, None, -10);
    let mut document = Document::open(&source).expect("fixture");
    let orphan = ObjectId::new();
    document
        .write(|w| {
            w.put_object(
                orphan,
                Some(first),
                -100,
                Some("No tip"),
                &ObjectPayload::Body(Body { tip_feature: None }),
            )
        })
        .expect("saved body without geometry");
    document.close().expect("close");
    let before = files(root.path());
    let reading = inspect(&source);
    assert_eq!(files(root.path()), before, "no kernel, writes or sidecars");
    assert_eq!(
        body_ids(&reading),
        [second, first, orphan],
        "parent, ordinal, UUID order; not name order"
    );
    assert!(
        reading["bodies"][0]
            .get("name")
            .expect("mandatory")
            .is_null()
    );
    assert_eq!(
        reading["features"], initial["features"],
        "Body discovery preserves the Extrude contract"
    );
    assert_ne!(reading["content_version"], initial["content_version"]);

    let pinned = Document::open_read_only(&source).expect("pinned reading");
    let common = ExtrudeEditSource::read(&pinned).expect("same snapshot");
    let catalog = stl_bodies(&pinned).expect("same snapshot");
    assert_eq!(
        reading["content_version"],
        common.version.content.to_string()
    );
    assert_eq!(
        reading["document_id"],
        pinned.meta().document_id.to_string()
    );
    for (wire, fact) in reading["bodies"]
        .as_array()
        .expect("bodies")
        .iter()
        .zip(&catalog)
    {
        assert_eq!(wire["body_id"], fact.id.to_string());
        assert_eq!(wire["name"], serde_json::json!(fact.name));
        assert!(matches!(
            pinned
                .object(fact.id)
                .expect("object")
                .expect("saved")
                .payload,
            ObjectPayload::Body(_)
        ));
        assert!(
            reading["features"]
                .as_array()
                .expect("features")
                .iter()
                .all(|f| f["feature_id"] != wire["body_id"])
        );
    }
    // Extend the existing read-only snapshot proof with Body facts while a
    // writer holds changed SQL rows. Neither catalog reads uncommitted state.
    let writer = rusqlite::Connection::open(&source).expect("writer");
    writer.execute_batch("BEGIN IMMEDIATE").expect("begin");
    assert_eq!(
        writer
            .execute(
                "UPDATE objects SET name='new saved name', ordinal=-20 WHERE id=?1",
                [first.to_bytes()]
            )
            .expect("changed row"),
        1
    );
    assert_eq!(stl_bodies(&pinned).expect("still pinned"), catalog);
    assert_eq!(
        ExtrudeEditSource::read(&pinned)
            .expect("still pinned")
            .version,
        common.version
    );
    let copy = root.path().join("pinned.fcad");
    pinned.snapshot_to(&copy).expect("snapshot");
    assert_eq!(
        inspect(&copy),
        reading,
        "metadata/version/features/bodies all belong to one snapshot"
    );
    pinned.close().expect("close");
    writer
        .execute_batch("COMMIT")
        .expect("publish writer after reading");
    drop(writer);
    let current = inspect(&source);
    assert_eq!(body_ids(&current), [first, second, orphan]);
    assert_eq!(current["bodies"][0]["name"], "new saved name");
    assert_ne!(current["content_version"], reading["content_version"]);
}

#[test]
fn export_refusals_and_flag_shaped_values_preserve_files_without_a_kernel() {
    let root = tempfile::tempdir().expect("dir");
    let source = root.path().join("source.fcad");
    let [first, _] = twins(&source);
    let id = first.to_string();
    let output = root.path().join("output.stl");
    for wanted in [
        None,
        Some(TWIN),
        Some("not-a-uuid"),
        Some(ObjectId::new().to_string().as_str()),
    ] {
        refusal(
            &mut export(&source, &output, wanted, true),
            "input",
            root.path(),
        );
    }
    // A syntactically valid Extrude UUID is still not a Body UUID.
    let feature = inspect(&source)["features"][0]["feature_id"]
        .as_str()
        .expect("feature")
        .to_owned();
    refusal(
        &mut export(&source, &output, Some(&feature), true),
        "input",
        root.path(),
    );
    for flag in ["linear-deflection", "angular-deflection"] {
        for value in ["NaN", "inf", "0", "-1"] {
            refusal(
                export(&source, &output, Some(&id), true).arg(format!("--{flag}={value}")),
                "input",
                root.path(),
            );
        }
    }
    refusal(
        &mut export(&root.path().join("missing.fcad"), &output, Some(&id), true),
        "io",
        root.path(),
    );
    let alias = root.path().join("source alias.stl");
    std::fs::hard_link(&source, &alias).expect("hardlink");
    for destination in [&source, &alias] {
        refusal(
            export(&source, destination, Some(&id), true).arg("--force"),
            "input",
            root.path(),
        );
    }
    #[cfg(unix)]
    {
        let link = root.path().join("source symlink.stl");
        std::os::unix::fs::symlink(&source, &link).expect("symlink");
        refusal(
            export(&source, &link, Some(&id), true).arg("--force"),
            "input",
            root.path(),
        );
    }
    std::fs::write(&output, b"keep existing destination").expect("sentinel");
    refusal(
        &mut export(&source, &output, Some(&id), true),
        "input",
        root.path(),
    );
    let names = files(root.path());
    // --solid is a string, so this is an execution error in text mode, not
    // a UUID clap error and not a request to turn on JSON globally.
    let value =
        run(export(&source, &root.path().join("absent.stl"), None, false).arg("--solid=--json"));
    assert_eq!(value.status.code(), Some(2));
    assert!(value.stdout.is_empty());
    assert!(
        String::from_utf8(value.stderr)
            .expect("text")
            .contains("error [input]")
    );
    assert_eq!(files(root.path()), names);

    let empty = root.path().join("--json");
    create(&empty, None);
    assert_eq!(inspect(&empty)["bodies"], serde_json::json!([]));
    let text = run(cli().current_dir(root.path()).args([
        "export-stl",
        "-o",
        "absent.stl",
        "--",
        "--json",
    ]));
    assert_eq!(text.status.code(), Some(2));
    assert!(text.stdout.is_empty() && !text.stderr.is_empty());
    refusal(
        &mut export(&empty, &root.path().join("empty.stl"), None, true),
        "input",
        root.path(),
    );

    // Stored import facts need no OCCT. This fixture promises no imported
    // geometry; a real STEP import is also covered by the native gate below.
    let imported = root.path().join("imported.fcad");
    let mut document = Document::create(&imported).expect("document");
    document
        .store_step_import(StepImportRequest {
            object: ObjectId::new(),
            name: Some("Imported only"),
            source: b"fixture STEP bytes",
            source_name: Some("fixture.step"),
            import: &ferritecad_exchange::Import::Imported {
                scene: ferritecad_exchange::Scene {
                    source_unit: "MM".into(),
                    schema: "AP242".into(),
                    definitions: vec![],
                    instances: vec![],
                },
                diagnostics: vec![],
            },
            importer: &ferritecad_kernel::KernelIdentity::new("fixture", "1", "test")
                .expect("identity"),
        })
        .expect("stored ImportedStep");
    document.close().expect("close");
    let before = files(root.path());
    assert_eq!(inspect(&imported)["bodies"], serde_json::json!([]));
    assert_eq!(files(root.path()), before);
    refusal(
        &mut export(&imported, &root.path().join("imported.stl"), None, true),
        "input",
        root.path(),
    );
    for sql in ["PRAGMA journal_mode=WAL;", "PRAGMA user_version=1;"] {
        let restricted = root.path().join(if sql.contains("WAL") {
            "wal.fcad"
        } else {
            "old.fcad"
        });
        create(&restricted, Some(["60", "40", "10"]));
        rusqlite::Connection::open(&restricted)
            .expect("fixture")
            .execute_batch(sql)
            .expect("read-only refusal");
        refusal(
            export(&restricted, &output, None, true).arg("--force"),
            "unsupported",
            root.path(),
        );
    }
}

fn published(
    source: &Path,
    destination: &Path,
    body: ObjectId,
    name: Option<&str>,
    height: f64,
) -> Vec<u8> {
    let before = std::fs::read(source).expect("source");
    let result = reply(
        &run(&mut export(
            source,
            destination,
            Some(&body.to_string()),
            true,
        )),
        "export-stl",
        0,
    );
    assert_eq!(
        result["result"],
        serde_json::json!({
            "destination": destination, "body_id": body, "body_name": name,
            "triangles": 12, "bytes": 684, "length_unit": "mm"
        })
    );
    let bytes = std::fs::read(destination).expect("success requires published STL");
    assert_eq!(bytes.len(), 684);
    let (bounds, volume) = stl_bounds(&bytes);
    assert_eq!(bounds, [60.0, 40.0, height]);
    assert!((volume - 60.0 * 40.0 * height).abs() < 1e-6);
    let text_path = destination.with_extension("text.stl");
    let text = success(&mut export(
        source,
        &text_path,
        Some(&body.to_string()),
        false,
    ));
    assert_eq!(std::fs::read(text_path).expect("text bytes"), bytes);
    assert!(
        String::from_utf8(text.stdout)
            .expect("text")
            .contains("(12 triangles, 684 bytes) from")
    );
    assert_eq!(std::fs::read(source).expect("unchanged"), before);
    bytes
}

/// Called inside the existing mandatory native JSON gate, never a green skip.
pub(super) fn native_contract() {
    let root = tempfile::tempdir().expect("dir");
    let source = root.path().join(if cfg!(unix) {
        "Плита \"А\"\nс пробелами.fcad"
    } else {
        "Плита с пробелами.fcad"
    });
    let [first, second] = twins(&source);
    let reading = inspect(&source);
    let ids = body_ids(&reading);
    assert_eq!(ids, [first, second]);
    let first_bytes = published(
        &source,
        &root.path().join("first.stl"),
        ids[0],
        Some(TWIN),
        10.0,
    );
    let second_bytes = published(
        &source,
        &root.path().join("second.stl"),
        ids[1],
        Some(TWIN),
        23.0,
    );
    assert_ne!(
        first_bytes, second_bytes,
        "two distinct geometries, not two serializer calls"
    );

    rename(&source, second, Some(&first.to_string()), -10);
    let changed = inspect(&source);
    assert_eq!(body_ids(&changed), [second, first]);
    assert_ne!(changed["content_version"], reading["content_version"]);
    published(
        &source,
        &root.path().join("id-priority.stl"),
        first,
        Some(TWIN),
        10.0,
    );
    // A selection made from the older inspect still addresses the same Body.
    published(
        &source,
        &root.path().join("reordered.stl"),
        second,
        Some(&first.to_string()),
        23.0,
    );
    rename(&source, second, None, -10);
    assert!(
        inspect(&source)["bodies"][0]
            .get("name")
            .expect("required name")
            .is_null()
    );
    published(
        &source,
        &root.path().join("unnamed.stl"),
        second,
        None,
        23.0,
    );

    // Both fresh publication and force replacement survive lost stdout, even
    // when stderr is also closed. The OS pipe is the same helper as edit/create.
    for replace in [false, true] {
        for both in [false, true] {
            let destination = root.path().join(format!("lost-{replace}-{both}.stl"));
            if replace {
                std::fs::write(&destination, &second_bytes).expect("old complete STL");
            }
            let mut command = export(&source, &destination, Some(&first.to_string()), true);
            if replace {
                command.arg("--force");
            }
            command.stdout(closed_pipe());
            if both {
                command.stderr(closed_pipe());
            }
            let before = files(root.path());
            let result = run(&mut command);
            assert_eq!(result.status.code(), Some(7), "{result:?}");
            assert!(result.stdout.is_empty());
            if both {
                assert!(result.stderr.is_empty());
            } else {
                assert!(
                    String::from_utf8(result.stderr)
                        .expect("diagnostic")
                        .contains("JSON report delivery failed")
                );
            }
            assert_eq!(
                std::fs::read(&destination).expect("published despite lost report"),
                first_bytes
            );
            let expected: Vec<_> = before
                .into_iter()
                .filter(|(name, _)| name != destination.file_name().expect("name"))
                .collect();
            let remaining: Vec<_> = files(root.path())
                .into_iter()
                .filter(|(name, _)| name != destination.file_name().expect("name"))
                .collect();
            assert_eq!(
                remaining, expected,
                "no retry, sidecars or collateral writes"
            );
        }
    }
    // Inspect/export have no cross-command version guard: changed geometry is
    // read now, and a deleted selection is not replaced with another Body.
    let mut document = Document::open(&source).expect("fixture");
    let ObjectPayload::Body(body) = document
        .object(second)
        .expect("body")
        .expect("stored")
        .payload
    else {
        unreachable!()
    };
    let feature = body.tip_feature.expect("tip");
    let mut object = document.object(feature).expect("feature").expect("stored");
    let ObjectPayload::Extrude(extrude) = &mut object.payload else {
        unreachable!()
    };
    extrude.end_condition = EndCondition::Blind {
        distance: Expression::constant(31.0).expect("height"),
    };
    document
        .write(|w| {
            w.put_object(
                feature,
                object.parent,
                object.ordinal,
                object.name.as_deref(),
                &object.payload,
            )
        })
        .expect("change saved geometry");
    document.close().expect("close");
    published(
        &source,
        &root.path().join("new-height.stl"),
        second,
        None,
        31.0,
    );
    let sql = rusqlite::Connection::open(&source).expect("fixture");
    sql.execute("DELETE FROM objects WHERE id=?1", [second.to_bytes()])
        .expect("remove selected Body");
    drop(sql);
    assert_eq!(body_ids(&inspect(&source)), [first]);
    refusal(
        export(
            &source,
            &root.path().join("first.stl"),
            Some(&second.to_string()),
            true,
        )
        .arg("--force"),
        "input",
        root.path(),
    );

    let imported = root.path().join("native import.fcad");
    success(
        cli()
            .arg("import-step")
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../fixtures/step/canonical/01-single-part.step"),
            )
            .arg("-o")
            .arg(&imported),
    );
    assert_eq!(inspect(&imported)["bodies"], serde_json::json!([]));
    refusal(
        &mut export(&imported, &root.path().join("not-native.stl"), None, true),
        "input",
        root.path(),
    );
}
