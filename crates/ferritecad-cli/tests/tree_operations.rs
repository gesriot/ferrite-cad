// SPDX-License-Identifier: MIT
use ferritecad_document::{Document, EditChange, FeatureAddition, KeepSelection, ObjectPayload};
use ferritecad_kernel::{GeometryKernel, OperationContext, TessellationParams};
use ferritecad_occt::OcctKernel;
use ferritecad_topology::generative::Name;
use ferritecad_types::{ObjectId, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use ferritecad_bench::corpus;
#[path = "../../ferritecad-fixtures/tests/support/naming.rs"]
mod naming;
#[path = "tree_operations/rollback.rs"]
mod rollback;

type Names = BTreeMap<Name, BTreeSet<Name>>;
struct Facts {
    scene: ferritecad_scene::LoadedScene,
    names: BTreeMap<ObjectId, Names>,
    volumes: BTreeMap<ObjectId, f64>,
}
fn facts(path: &Path) -> Result<Facts> {
    let doc = Document::open_read_only(path)?;
    let mut kernel = OcctKernel::new()?;
    let ctx = OperationContext::default();
    let built = ferritecad_eval::rebuild_cold(&doc, &mut kernel, &ctx)?;
    measured(&doc, built, &mut kernel)
}
fn measured(
    doc: &Document,
    built: ferritecad_eval::RebuildResult,
    kernel: &mut OcctKernel,
) -> Result<Facts> {
    assert!(
        built.all_built(),
        "{}: {:?}",
        doc.path().display(),
        built.report()
    );
    let ctx = OperationContext::default();
    let mut names = BTreeMap::new();
    let mut volumes = BTreeMap::new();
    for (id, named) in &built.topology().features {
        if doc.object(*id)?.expect("named feature").suppressed {
            continue;
        }
        names.insert(*id, naming::signature(named));
        volumes.insert(*id, kernel.shape_stats(built.shape(*id).expect("shape"))?.1);
    }
    let scene = ferritecad_scene::snapshot_from_rebuild(
        doc,
        &built,
        kernel,
        |_, _| unreachable!("native bodies"),
        &TessellationParams::default(),
        &ctx,
    )?;
    built.release_all(kernel);
    assert_eq!(kernel.live_shape_count(), 0);
    Ok(Facts {
        scene,
        names,
        volumes,
    })
}
fn equivalent(a: &Facts, b: &Facts) {
    assert_eq!(a.names, b.names, "complete feature name signatures");
    assert_eq!(
        a.volumes.keys().collect::<Vec<_>>(),
        b.volumes.keys().collect::<Vec<_>>()
    );
    for (id, value) in &a.volumes {
        assert!((value - b.volumes[id]).abs() <= 1e-8, "volume {id}");
    }
    assert_eq!(
        a.scene.snapshot, b.scene.snapshot,
        "geometry and placements"
    );
    assert_eq!(a.scene.catalogue, b.scene.catalogue, "Body identities");
    assert_eq!(a.scene.faces, b.scene.faces, "face meanings");
    assert_eq!(a.scene.edges, b.scene.edges, "edge meanings");
    assert_eq!(a.scene.vertices, b.scene.vertices, "vertex meanings");
}
fn edit(source: &Path, destination: &Path, changes: &[EditChange]) -> Result<serde_json::Value> {
    let request = destination.with_extension("json");
    std::fs::write(
        &request,
        ferritecad_fixtures::edit_request_json(changes).to_string(),
    )?;
    let doc = Document::open_read_only(source)?;
    let version = doc.content_version()?.to_string();
    doc.close()?;
    let before = std::fs::read(source)?;
    let entries = std::fs::read_dir(destination.parent().expect("directory"))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<BTreeSet<_>>>()?;
    let output = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
        .arg("edit")
        .arg(source)
        .arg("--expect-version")
        .arg(version)
        .arg("--request")
        .arg(&request)
        .arg("--output")
        .arg(destination)
        .arg("--json")
        .output()?;
    assert_eq!(std::fs::read(source)?, before);
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON response");
    if !output.status.success() {
        assert!(!destination.exists(), "refused candidate was published");
        assert_eq!(
            std::fs::read_dir(destination.parent().expect("directory"))?
                .map(|e| e.map(|e| e.path()))
                .collect::<std::io::Result<BTreeSet<_>>>()?,
            entries
        );
    }
    Ok(result)
}
fn accepted(value: serde_json::Value) {
    assert_eq!(value["ok"], true, "{value}");
}
fn suppression(source: &Path, feature: ObjectId, root: &Path, original: &Facts) -> Result<bool> {
    let removed = root.join("removed.fcad");
    let result = edit(
        source,
        &removed,
        &[EditChange::RemoveObject { object: feature }],
    )?;
    if result["ok"] != true {
        return Ok(false);
    }
    let suppressed = root.join("suppressed.fcad");
    let result = edit(
        source,
        &suppressed,
        &[EditChange::SetSuppressed {
            object: feature,
            suppressed: true,
        }],
    )?;
    assert_eq!(
        result["result"]["rebuild"]["suppressed"],
        serde_json::json!([feature]),
        "{result}"
    );
    accepted(result);
    equivalent(&facts(&removed)?, &facts(&suppressed)?);
    let restored = root.join("restored.fcad");
    accepted(edit(
        &suppressed,
        &restored,
        &[EditChange::SetSuppressed {
            object: feature,
            suppressed: false,
        }],
    )?);
    equivalent(original, &facts(&restored)?);
    Ok(true)
}
#[test]
fn native_middle_and_tip_suppression_equal_removal() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&source)?;
    let before = root.path().join("before.fcad");
    std::fs::copy(&source, &before)?;
    let mut doc = Document::open(&source)?;
    let body = ferritecad_fixtures::feature_changes::body(&doc)?;
    let (base, _, _) = ferritecad_fixtures::feature_changes::base_profile(&doc, body)?;
    let mut features = Vec::new();
    let mut second = Vec::new();
    for x in [10., 30.] {
        let tool = ferritecad_fixtures::feature_changes::circular_tool(
            &doc,
            body,
            [x, 15.],
            2.,
            ferritecad_document::EndCondition::ThroughAll,
        )?;
        features.push(tool.changes.last().expect("feature").object());
        doc.write_changes(&tool.changes)?;
        if x == 30. {
            second = tool.changes;
        }
    }
    doc.close()?;
    let never_added = root.path().join("never-added.fcad");
    accepted(edit(&before, &never_added, &second)?);
    let removed_middle = root.path().join("removed-middle.fcad");
    accepted(edit(
        &source,
        &removed_middle,
        &[EditChange::RemoveObject {
            object: features[0],
        }],
    )?);
    let removed = Document::open_read_only(&removed_middle)?;
    assert_eq!(
        removed
            .object(features[1])?
            .expect("successor")
            .payload
            .previous_feature(),
        Some(base)
    );
    let ObjectPayload::Body(part) = removed.object(body)?.expect("Body").payload else {
        unreachable!("Body")
    };
    assert_eq!(part.tip_feature, Some(features[1]));
    assert!(
        removed
            .dependencies()?
            .contains(&ferritecad_document::Dependency {
                dependent: features[1],
                dependency: base,
                role: ferritecad_document::DependencyRole::Predecessor,
            })
    );
    equivalent(&facts(&never_added)?, &facts(&removed_middle)?);
    let original = facts(&source)?;
    for feature in features {
        let dir = tempfile::tempdir_in(root.path())?;
        assert!(suppression(&source, feature, dir.path(), &original)?);
    }
    Ok(())
}
fn corpus_paths(root: &Path) -> Result<Vec<(String, std::path::PathBuf)>> {
    let mut paths: Vec<_> = corpus::generate(root, false)?
        .into_iter()
        // Every case rebuilds its whole document. The fifty bodies repeat the ten of
        // `10_bodies`, so they would add most of the run time and no new case.
        .filter(|c| c.name != "50_bodies")
        .map(|c| (c.name, c.path))
        .collect();
    for name in ["chamfer", "open_slot", "overlapping_tools", "pocket"] {
        let path = root.join(format!("{name}.fcad"));
        ferritecad_fixtures::write_plate(&path)?;
        let mut doc = Document::open(&path)?;
        let body = ferritecad_fixtures::feature_changes::body(&doc)?;
        if name == "chamfer" {
            let change =
                ferritecad_fixtures::feature_changes::edge_addition(&doc, body, 0, 2., true)?;
            doc.write_changes(&[change])?;
        } else {
            let end = if name == "pocket" {
                ferritecad_document::EndCondition::Blind {
                    distance: ferritecad_document::Expression::constant(4.)?,
                }
            } else {
                ferritecad_document::EndCondition::ThroughAll
            };
            let center = if name == "open_slot" {
                [1., 20.]
            } else {
                [20., 20.]
            };
            let tool =
                ferritecad_fixtures::feature_changes::circular_tool(&doc, body, center, 2., end)?;
            doc.write_changes(&tool.changes)?;
            if name == "overlapping_tools" {
                let next = ferritecad_fixtures::feature_changes::circular_tool(
                    &doc,
                    body,
                    [22., 20.],
                    2.,
                    ferritecad_document::EndCondition::ThroughAll,
                )?;
                doc.write_changes(&next.changes)?;
            }
        }
        doc.close()?;
        paths.push((name.into(), path));
    }
    assert_eq!(paths.len(), 13);
    let acceptance = root.join("live-acceptance.fcad");
    ferritecad_fixtures::live_model::write(&acceptance)?;
    paths.push(("live-acceptance".into(), acceptance));
    Ok(paths)
}

#[test]
#[allow(
    clippy::panic,
    reason = "identify the exact corpus object whose closure refuses"
)]
fn native_dependent_closures_are_accepted_by_the_edit_transaction() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let mut kernel = OcctKernel::new()?;
    for (name, source) in corpus_paths(root.path())? {
        if name == "plate_4_fillets_constraints"
            && !ferritecad_fixtures::native_dependencies(&[(
                ferritecad_sketch_solver::is_available(),
                "PLANEGCS",
            )])
        {
            continue;
        }
        let doc = Document::open_read_only(&source)?;
        let expected = ferritecad_document::DocumentVersion {
            document_id: doc.meta().document_id,
            content: doc.content_version()?,
        };
        for object in doc.objects()?.into_iter().filter(|o| {
            matches!(o.payload, ObjectPayload::Sketch(_))
                || o.payload.kind().is_some_and(|k| k.is_feature())
        }) {
            let ids = doc.dependent_closure(object.id)?;
            let changes = ids
                .iter()
                .map(|id| EditChange::RemoveObject { object: *id })
                .collect();
            let destination = root.path().join(format!("removed-{}.fcad", object.id));
            let before = std::fs::read(&source)?;
            eprintln!(
                "{name}: remove {} {:?}, closure {ids:?}",
                object.id, object.name
            );
            let result = ferritecad_jobs::edit_copy(
                &ferritecad_jobs::EditRequest {
                    source: source.clone(),
                    expected,
                    changes,
                    destination: destination.clone(),
                },
                &mut kernel,
                &OperationContext::default(),
            );
            assert_eq!(std::fs::read(&source)?, before);
            assert_eq!(kernel.live_shape_count(), 0);
            let edited = result.unwrap_or_else(|error| {
                panic!(
                    "{name}: closure of {} {:?} refused: {error}",
                    object.id, object.name
                )
            });
            assert!(edited.rebuild.failed.is_empty());
            assert!(edited.rebuild.skipped.is_empty());
            let candidate = Document::open_read_only(&destination)?;
            for id in ids {
                assert!(candidate.object(id)?.is_none());
            }
            candidate.close()?;
            std::fs::remove_file(destination)?;
        }
    }
    Ok(())
}

#[test]
fn native_cli_removes_one_complete_dependent_closure() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let source = root.path().join("plate.fcad");
    ferritecad_fixtures::write_plate(&source)?;
    let doc = Document::open_read_only(&source)?;
    let sketch = doc
        .objects()?
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("plate profile")
        .id;
    let closure = doc.dependent_closure(sketch)?;
    let changes: Vec<_> = closure
        .iter()
        .map(|id| EditChange::RemoveObject { object: *id })
        .collect();
    doc.close()?;
    let output = root.path().join("removed.fcad");
    assert_eq!(edit(&source, &output, &changes)?["ok"], true);
    let result = Document::open_read_only(&output)?;
    for id in closure {
        assert!(result.object(id)?.is_none());
    }
    assert!(result.objects()?.iter().any(|o| matches!(&o.payload,
        ObjectPayload::Body(body) if body.tip_feature.is_none())));
    Ok(())
}

fn edit_in_process(
    kernel: &mut OcctKernel,
    source: &Path,
    destination: &Path,
    changes: &[EditChange],
) -> Result<ferritecad_jobs::EditedDocument> {
    let document = Document::open_read_only(source)?;
    let expected = ferritecad_document::DocumentVersion {
        document_id: document.meta().document_id,
        content: document.content_version()?,
    };
    document.close()?;
    let before = std::fs::read(source)?;
    let entries = std::fs::read_dir(destination.parent().expect("directory"))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<BTreeSet<_>>>()?;
    let result = ferritecad_jobs::edit_copy(
        &ferritecad_jobs::EditRequest {
            source: source.to_path_buf(),
            expected,
            changes: changes.to_vec(),
            destination: destination.to_path_buf(),
        },
        kernel,
        &OperationContext::default(),
    );
    assert_eq!(std::fs::read(source)?, before);
    assert_eq!(kernel.live_shape_count(), 0);
    if result.is_err() {
        assert!(!destination.exists(), "refused candidate was published");
        assert_eq!(
            std::fs::read_dir(destination.parent().expect("directory"))?
                .map(|e| e.map(|e| e.path()))
                .collect::<std::io::Result<BTreeSet<_>>>()?,
            entries
        );
    }
    result
}

fn suppression_in_process(
    kernel: &mut OcctKernel,
    source: &Path,
    feature: ObjectId,
    root: &Path,
    original: &Facts,
) -> Result<bool> {
    let removed = root.join("removed.fcad");
    if edit_in_process(
        kernel,
        source,
        &removed,
        &[EditChange::RemoveObject { object: feature }],
    )
    .is_err()
    {
        return Ok(false);
    }
    let suppressed = root.join("suppressed.fcad");
    let result = edit_in_process(
        kernel,
        source,
        &suppressed,
        &[EditChange::SetSuppressed {
            object: feature,
            suppressed: true,
        }],
    )?;
    assert_eq!(result.rebuild.suppressed, vec![feature]);
    equivalent(&facts(&removed)?, &facts(&suppressed)?);
    let restored = root.join("restored.fcad");
    edit_in_process(
        kernel,
        &suppressed,
        &restored,
        &[EditChange::SetSuppressed {
            object: feature,
            suppressed: false,
        }],
    )?;
    equivalent(original, &facts(&restored)?);
    Ok(true)
}

#[test]
fn native_corpus_and_live_acceptance_preserve_removal_and_suppression_invariants() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let paths = corpus_paths(root.path())?;
    let mut kernel = OcctKernel::new()?;
    for (name, source) in paths {
        // A constrained corpus case requires the native solver in addition to OCCT.
        if name == "plate_4_fillets_constraints"
            && !ferritecad_fixtures::native_dependencies(&[(
                ferritecad_sketch_solver::is_available(),
                "PLANEGCS",
            )])
        {
            continue;
        }
        let original = facts(&source)?;
        let doc = Document::open_read_only(&source)?;
        let objects = doc.objects()?;
        for body in objects
            .iter()
            .filter(|o| matches!(o.payload, ObjectPayload::Body(_)))
        {
            let dir = tempfile::tempdir_in(root.path())?;
            let appended = dir.path().join("appended.fcad");
            let feature = ObjectId::new();
            edit_in_process(
                &mut kernel,
                &source,
                &appended,
                &[EditChange::AddFeature {
                    object: feature,
                    body: body.id,
                    name: Some("Keep every solid".into()),
                    feature: FeatureAddition::KeepSolids {
                        selection: KeepSelection::All,
                    },
                }],
            )?;
            let restored = dir.path().join("original.fcad");
            edit_in_process(
                &mut kernel,
                &appended,
                &restored,
                &[EditChange::RemoveObject { object: feature }],
            )?;
            equivalent(&original, &facts(&restored)?);
        }
        let mut accepted_count = 0;
        for object in objects
            .iter()
            .filter(|o| o.payload.kind().is_some_and(|k| k.is_feature()))
        {
            let dir = tempfile::tempdir_in(root.path())?;
            eprintln!("{name}: {} {:?}", object.id, object.name);
            accepted_count += usize::from(suppression_in_process(
                &mut kernel,
                &source,
                object.id,
                dir.path(),
                &original,
            )?);
        }
        assert!(accepted_count > 0, "{name} has a removable feature");
    }
    Ok(())
}

#[test]
fn native_consumed_names_refuse_removal_and_suppression_with_every_new_failure() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    use ferritecad_document::{
        EndCondition, EntityKind, EntityReference, Expression, ReferenceMeaning, Selection,
    };
    use ferritecad_kernel::{NamingOperation, SubShapeKind};
    use ferritecad_topology::generative::{cap_root, curve_root, generated};
    let root = tempfile::tempdir()?;
    let source = root.path().join("pocket.fcad");
    ferritecad_fixtures::write_plate(&source)?;
    let mut doc = Document::open(&source)?;
    let body = ferritecad_fixtures::feature_changes::body(&doc)?;
    let (_, _, profile) = ferritecad_fixtures::feature_changes::base_profile(&doc, body)?;
    let tool = ferritecad_fixtures::feature_changes::circular_tool(
        &doc,
        body,
        [20., 20.],
        5.,
        EndCondition::Blind {
            distance: Expression::constant(4.)?,
        },
    )?;
    doc.write_changes(&tool.changes)?;
    let mut kernel = OcctKernel::new()?;
    let built = ferritecad_eval::rebuild_cold(&doc, &mut kernel, &OperationContext::default())?;
    assert!(built.all_built());
    let named = &built.topology().features[&tool.feature];
    let wall = named.resolve(generated(
        tool.feature,
        NamingOperation::Extrude,
        curve_root(tool.feature, tool.curve, Some(profile.plane)),
        SubShapeKind::Face,
    ))?;
    let floor = named.resolve(generated(
        tool.feature,
        NamingOperation::Extrude,
        cap_root(tool.feature, true, Some(profile.plane)),
        SubShapeKind::Face,
    ))?;
    let rims: Vec<_> = named
        .topology
        .entities(SubShapeKind::Edge)
        .filter(|edge| {
            let faces = &named.topology.adjacent_faces[edge];
            faces.contains(&wall) && faces.contains(&floor)
        })
        .collect();
    assert_eq!(rims.len(), 1);
    let reference = EntityReference::new(ReferenceMeaning {
        producer: tool.feature,
        kind: EntityKind::Edge,
        digest: named.entities[&rims[0]].digest,
        selection: Selection::One,
    });
    built.release_all(&mut kernel);
    let first = ObjectId::new();
    doc.write_changes(&[EditChange::AddFeature {
        object: first,
        body,
        name: Some("Round the pocket floor".into()),
        feature: FeatureAddition::Fillet {
            edges: vec![reference],
            radius_mm: Expression::constant(0.5)?,
        },
    }])?;
    let next = ferritecad_fixtures::feature_changes::edge_addition(&doc, body, 0, 1., false)?;
    let second = next.object();
    doc.write_changes(&[next])?;
    doc.close()?;
    let original = facts(&source)?;
    assert_eq!(original.volumes.len(), 4);
    for change in [
        EditChange::RemoveObject {
            object: tool.feature,
        },
        EditChange::SetSuppressed {
            object: tool.feature,
            suppressed: true,
        },
    ] {
        let dir = tempfile::tempdir_in(root.path())?;
        let result = edit(&source, &dir.path().join("refused.fcad"), &[change])?;
        assert_eq!(result["ok"], false, "{result}");
        let report = &result["error"]["rebuild"];
        assert!(
            report["failed"]
                .as_array()
                .expect("failures")
                .iter()
                .any(|r| r["id"] == serde_json::json!(first)),
            "{result}"
        );
        assert!(
            report["skipped"]
                .as_array()
                .expect("dependents")
                .iter()
                .any(|r| r["id"] == serde_json::json!(second)),
            "{result}"
        );
        assert!(
            result.to_string().contains(&reference.id.to_string()),
            "{result}"
        );
    }
    Ok(())
}

#[test]
fn native_suppressed_root_is_empty_and_exports_like_a_removed_root() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&source)?;
    let doc = Document::open_read_only(&source)?;
    let body = ferritecad_fixtures::feature_changes::body(&doc)?;
    let (base, _, _) = ferritecad_fixtures::feature_changes::base_profile(&doc, body)?;
    doc.close()?;
    let mut reports = Vec::new();
    for (name, change) in [
        ("removed", EditChange::RemoveObject { object: base }),
        (
            "suppressed",
            EditChange::SetSuppressed {
                object: base,
                suppressed: true,
            },
        ),
    ] {
        let path = root.path().join(format!("{name}.fcad"));
        accepted(edit(&source, &path, &[change])?);
        assert!(facts(&path)?.scene.snapshot.is_empty());
        let mut kernel = OcctKernel::new()?;
        let exported = ferritecad_scene::export_scene(
            &path,
            &mut kernel,
            |_, _| unreachable!("native Body"),
            &TessellationParams::default(),
            &OperationContext::default(),
        )?;
        assert!(exported.completeness().is_complete());
        assert!(exported.definitions().is_empty());
        assert!(exported.nodes().is_empty());
        let out = root.path().join(format!("{name}.stl"));
        let result = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
            .arg("export-stl")
            .arg(&path)
            .arg("--solid")
            .arg(body.to_string())
            .arg("-o")
            .arg(&out)
            .arg("--json")
            .output()?;
        assert!(!out.exists());
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).expect("export JSON");
        reports.push((
            result.status.code(),
            value["ok"].clone(),
            value["result"]["complete"].clone(),
            value["result"]["triangles"].clone(),
        ));
        if name == "suppressed" {
            let result = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
                .arg("rebuild")
                .arg(&path)
                .arg("--cold")
                .output()?;
            assert!(result.status.success(), "{result:?}");
            assert!(String::from_utf8_lossy(&result.stdout).contains("Suppressed"));
        }
    }
    assert_eq!(reports[0], reports[1]);
    assert_eq!(
        reports[0],
        (
            Some(6),
            serde_json::json!(true),
            serde_json::json!(false),
            serde_json::json!(0)
        )
    );
    Ok(())
}

#[test]
fn native_suppression_changes_cache_inputs_and_restores_the_original_names() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&source)?;
    let mut doc = Document::open(&source)?;
    let body = ferritecad_fixtures::feature_changes::body(&doc)?;
    let tool = ferritecad_fixtures::feature_changes::circular_tool(
        &doc,
        body,
        [20., 20.],
        2.,
        ferritecad_document::EndCondition::ThroughAll,
    )?;
    doc.write_changes(&tool.changes)?;
    let successor = ferritecad_fixtures::feature_changes::circular_tool(
        &doc,
        body,
        [40., 20.],
        2.,
        ferritecad_document::EndCondition::ThroughAll,
    )?;
    doc.write_changes(&successor.changes)?;
    let mut kernel = OcctKernel::new()?;
    let mut cache = ferritecad_document::CacheStore::open(
        root.path().join("cache"),
        doc.meta().document_id,
        kernel.identity().id(),
        kernel.identity().version(),
    )?;
    let ctx = OperationContext::default();
    for suppressed in [false, true, false] {
        doc.write_changes(&[EditChange::SetSuppressed {
            object: tool.feature,
            suppressed,
        }])?;
        let cold = facts(&source)?;
        for attempt in 0..2 {
            let (built, events) =
                ferritecad_eval::rebuild_cached(&doc, &mut kernel, &mut cache, &ctx)?;
            assert_eq!(
                built.report().suppressed.contains(&tool.feature),
                suppressed
            );
            if attempt == 1 {
                assert!(!events.is_empty());
                assert!(
                    events
                        .iter()
                        .all(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit)
                );
            }
            equivalent(&cold, &measured(&doc, built, &mut kernel)?);
        }
    }
    Ok(())
}

#[test]
fn native_suppression_bypasses_own_region_failure_but_refuses_an_empty_predecessor() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&source)?;
    let mut doc = Document::open(&source)?;
    let body = ferritecad_fixtures::feature_changes::body(&doc)?;
    let (base, _, _) = ferritecad_fixtures::feature_changes::base_profile(&doc, body)?;
    let tool = ferritecad_fixtures::feature_changes::circular_tool(
        &doc,
        body,
        [20., 20.],
        2.,
        ferritecad_document::EndCondition::ThroughAll,
    )?;
    doc.write_changes(&tool.changes)?;
    doc.close()?;
    let refused = edit(
        &source,
        &root.path().join("refused.fcad"),
        &[EditChange::SetSuppressed {
            object: base,
            suppressed: true,
        }],
    )?;
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(
        refused["error"]["rebuild"]["failed"]
            .as_array()
            .expect("failures")
            .iter()
            .any(|r| r["id"] == serde_json::json!(tool.feature)),
        "{refused}"
    );
    assert!(refused.to_string().contains("no solid result"), "{refused}");
    let mut doc = Document::open(&source)?;
    doc.write_changes(&[EditChange::SetRegions {
        object: tool.feature,
        regions: vec![ferritecad_types::StableEntityId::new()],
    }])?;
    doc.close()?;
    let target = root.path().join("repaired.fcad");
    accepted(edit(
        &source,
        &target,
        &[EditChange::SetSuppressed {
            object: tool.feature,
            suppressed: true,
        }],
    )?);
    let scene = facts(&target)?;
    assert_eq!(scene.volumes.len(), 1);
    assert!((scene.volumes[&base] - 24000.).abs() <= 1e-8);
    Ok(())
}

#[test]
fn native_cli_batch_removal_reports_every_user_and_keeps_an_empty_body() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    use ferritecad_document::{EndCondition, Expression, ExpressionSlot};
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.fcad");
    ferritecad_fixtures::write_plate(&source)?;
    let mut doc = Document::open(&source)?;
    let body = ferritecad_fixtures::feature_changes::body(&doc)?;
    let (base, profile, _) = ferritecad_fixtures::feature_changes::base_profile(&doc, body)?;
    let parameter = ObjectId::new();
    doc.write_changes(&[
        EditChange::AddParameter {
            object: parameter,
            name: "height".into(),
            dimension: ferritecad_types::Dimension::Length,
            expression: "10 mm".into(),
        },
        EditChange::SetExpressionText {
            object: base,
            slot: ExpressionSlot::ExtrudeDistance,
            source: "height".into(),
        },
    ])?;
    let mut features = vec![base];
    for x in [10., 30.] {
        let tool = ferritecad_fixtures::feature_changes::circular_tool(
            &doc,
            body,
            [x, 15.],
            2.,
            EndCondition::Blind {
                distance: Expression::constant(10.)?,
            },
        )?;
        doc.write_changes(&tool.changes)?;
        features.push(tool.feature);
    }
    doc.write_changes(&[EditChange::SetExpressionText {
        object: features[2],
        slot: ExpressionSlot::ExtrudeDistance,
        source: "height".into(),
    }])?;
    let ids: Vec<_> = doc.objects()?.into_iter().map(|o| o.id).collect();
    doc.close()?;
    facts(&source)?;
    for (objects, users, words) in [
        (vec![profile], vec![base], vec!["sketch", "Extrude"]),
        (
            vec![parameter],
            vec![base, features[2]],
            vec!["parameter", "height", "Extrude"],
        ),
        (
            vec![body],
            features.clone(),
            vec!["body", "history feature"],
        ),
        (
            vec![base],
            vec![features[1]],
            vec!["predecessor", "Extrude"],
        ),
        (
            vec![profile, parameter],
            vec![base, features[2]],
            vec!["sketch", "parameter"],
        ),
    ] {
        let dir = tempfile::tempdir_in(root.path())?;
        let changes: Vec<_> = objects
            .into_iter()
            .map(|object| EditChange::RemoveObject { object })
            .collect();
        let report = edit(&source, &dir.path().join("refused.fcad"), &changes)?;
        assert_eq!(report["ok"], false, "{report}");
        assert_eq!(report["error"]["kind"], "input", "{report}");
        let text = report.to_string();
        for id in users {
            assert!(text.contains(&id.to_string()), "{report}");
        }
        for word in words {
            assert!(text.contains(word), "{report}");
        }
    }
    let empty = root.path().join("empty-body.fcad");
    let changes: Vec<_> = features
        .into_iter()
        .map(|object| EditChange::RemoveObject { object })
        .collect();
    accepted(edit(&source, &empty, &changes)?);
    let doc = Document::open_read_only(&empty)?;
    let ObjectPayload::Body(part) = doc.object(body)?.expect("retained Body").payload else {
        unreachable!("Body")
    };
    assert_eq!(part.tip_feature, None);
    assert!(facts(&empty)?.scene.snapshot.is_empty());
    let gone = root.path().join("gone.fcad");
    let changes: Vec<_> = ids
        .into_iter()
        .map(|object| EditChange::RemoveObject { object })
        .collect();
    accepted(edit(&source, &gone, &changes)?);
    let doc = Document::open_read_only(&gone)?;
    assert!(doc.objects()?.is_empty());
    assert!(doc.dependencies()?.is_empty());
    assert!(facts(&gone)?.scene.snapshot.is_empty());
    Ok(())
}

#[test]
fn native_cli_import_removal_reclaims_only_the_last_source_claim() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let root = tempfile::tempdir()?;
    let input = root.path().join("single.step");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/step/canonical/01-single-part.step"),
        &input,
    )?;
    let bytes = std::fs::read(&input)?;
    let source = root.path().join("import.fcad");
    let output = Command::new(env!("CARGO_BIN_EXE_ferritecad"))
        .arg("import-step")
        .arg(&input)
        .arg("-o")
        .arg(&source)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(std::fs::read(&input)?, bytes);
    let mut doc = Document::open(&source)?;
    let first = doc
        .objects()?
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::ImportedStep(_)))
        .expect("imported object");
    let second = ObjectId::new();
    let mut kernel = OcctKernel::new()?;
    let imported = kernel.import_step(&bytes)?;
    doc.store_step_import(ferritecad_document::StepImportRequest {
        object: second,
        name: Some("Second claim"),
        source: &bytes,
        source_name: None,
        import: &imported,
        importer: kernel.identity(),
    })?;
    for definition in &imported.scene().expect("imported scene").definitions {
        kernel.release(definition.shape);
    }
    assert_eq!(kernel.live_shape_count(), 0);
    doc.close()?;
    let refused = edit(
        &source,
        &root.path().join("refused.fcad"),
        &[EditChange::SetSuppressed {
            object: first.id,
            suppressed: true,
        }],
    )?;
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"]["kind"], "input", "{refused}");
    assert!(refused.to_string().contains("imported STEP"), "{refused}");
    let mut previous = source;
    for (id, remaining) in [(first.id, 1), (second, 0)] {
        let path = root.path().join(format!("after-{remaining}.fcad"));
        accepted(edit(
            &previous,
            &path,
            &[EditChange::RemoveObject { object: id }],
        )?);
        let doc = Document::open_read_only(&path)?;
        assert_eq!(doc.objects()?.len(), remaining);
        doc.close()?;
        let sql = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .expect("read candidate");
        let (sources, claims): (i64, i64) = sql
            .query_row(
                "SELECT (SELECT count(*) FROM imported_sources),
                    (SELECT count(*) FROM imported_source_refs)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("source reachability");
        assert_eq!((sources, claims), (remaining as i64, remaining as i64));
        previous = path;
    }
    Ok(())
}

#[test]
fn native_suppression_ignores_failed_profile_but_requires_a_built_predecessor() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    use ferritecad_document::{
        DatumPlane, EndCondition, EntityKind, EntityReference, Expression, ReferenceMeaning,
        Selection,
    };
    let root = tempfile::tempdir()?;
    let source = root.path().join("failed-profile.fcad");
    ferritecad_fixtures::write_plate(&source)?;
    let mut doc = Document::open(&source)?;
    let body = ferritecad_fixtures::feature_changes::body(&doc)?;
    let (base, profile, _) = ferritecad_fixtures::feature_changes::base_profile(&doc, body)?;
    // Another suppressed result must not hide this feature's skipped status.
    let independent = ObjectId::new();
    doc.write_changes(&[
        EditChange::AddBody {
            object: ObjectId::new(),
            name: Some("Independent".into()),
            feature_object: independent,
            feature_name: Some("Suppressed root".into()),
            feature: FeatureAddition::Extrude {
                profile,
                regions: vec![],
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(1.)?,
                },
                reversed: false,
                operation: ferritecad_document::SolidOperation::NewBody,
            },
        },
        EditChange::SetSuppressed {
            object: independent,
            suppressed: true,
        },
    ])?;
    let plane = ObjectId::new();
    let mut tool = ferritecad_fixtures::feature_changes::circular_tool(
        &doc,
        body,
        [20., 20.],
        2.,
        EndCondition::ThroughAll,
    )?;
    let EditChange::AddSketch { sketch, .. } = &mut tool.changes[0] else {
        unreachable!("sketch")
    };
    sketch.plane = plane;
    let keep = ObjectId::new();
    let mut changes = vec![EditChange::AddDatumPlane {
        object: plane,
        name: Some("Lost face".into()),
        plane: DatumPlane::Face {
            result: base,
            face: EntityReference::new(ReferenceMeaning {
                producer: base,
                kind: EntityKind::Face,
                digest: ferritecad_types::ContentHash::of_bytes(b"no such face"),
                selection: Selection::One,
            }),
            offset_mm: Expression::constant(0.)?,
        },
    }];
    changes.extend(tool.changes);
    changes.push(EditChange::AddFeature {
        object: keep,
        body,
        name: Some("Keep after failed Cut".into()),
        feature: FeatureAddition::KeepSolids {
            selection: KeepSelection::All,
        },
    });
    doc.write_changes(&changes)?;
    doc.close()?;
    for (feature, suppressed) in [(keep, false), (tool.feature, true)] {
        let target = root.path().join(format!("suppressed-{feature}.fcad"));
        let result = edit(
            &source,
            &target,
            &[EditChange::SetSuppressed {
                object: feature,
                suppressed: true,
            }],
        )?;
        assert_eq!(result["ok"], true, "{result}");
        let report = &result["result"]["rebuild"];
        assert!(
            report["suppressed"]
                .as_array()
                .expect("independent suppressed result")
                .contains(&serde_json::json!(independent))
        );
        assert_eq!(
            report["suppressed"]
                .as_array()
                .expect("statuses")
                .contains(&serde_json::json!(feature)),
            suppressed,
            "{result}"
        );
        assert_eq!(
            report["skipped"]
                .as_array()
                .expect("skipped")
                .iter()
                .any(|row| row["id"] == serde_json::json!(feature)),
            !suppressed,
            "{result}"
        );
        assert!(
            report["failed"]
                .as_array()
                .expect("failed")
                .iter()
                .any(|row| row["id"] == serde_json::json!(plane)),
            "{result}"
        );
        if suppressed {
            assert!(
                !report["skipped"]
                    .as_array()
                    .expect("skipped")
                    .iter()
                    .any(|row| row["id"] == serde_json::json!(keep)),
                "{result}"
            );
        }
    }
    Ok(())
}
