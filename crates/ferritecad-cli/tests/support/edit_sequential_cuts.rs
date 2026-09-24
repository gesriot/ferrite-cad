// SPDX-License-Identifier: MIT
//! §26D: the public editor, measured through the same independent §26C probes.
use super::*;
use ferritecad_document::{CircularCutEdit, SavedCircularCut};
use ferritecad_jobs::{EditCircularCutRequest, edit_circular_cut_copy};
const EDIT: &str = "edit-circular-cut";

fn two(root: &Path, tools: [CircularCut; 2]) -> PathBuf {
    let one = first(root, tools[0], false);
    let two = root.join("two.fcad");
    std::fs::copy(one, &two).expect("copy");
    let mut d = Document::open(&two).expect("doc");
    let (body, _) = tip(&d);
    let p = ferritecad_document::prepare_circular_cut(&d, body, &tools[1]).expect("second");
    d.write_circular_cut(&p).expect("write");
    d.close().expect("close");
    // Physical SQL row order, ordinal and all display names are misleading.
    let sql = rusqlite::Connection::open(&two).expect("SQL");
    sql.execute_batch(
        "UPDATE objects SET rowid=-rowid,ordinal=100-ordinal,name='same';
        UPDATE capabilities SET rowid=rowid+100;
        INSERT INTO capabilities(rowid,name,required) VALUES(900,'optional.extension',0);
        CREATE TABLE extra_data(id INTEGER PRIMARY KEY, bytes BLOB);
        INSERT INTO extra_data VALUES(91,x'010203');",
    )
    .expect("metadata");
    two
}

fn selected(path: &Path, index: usize) -> SavedCircularCut {
    let d = Document::open_read_only(path).expect("doc");
    let (_, last) = tip(&d);
    let ObjectPayload::Extrude(e) = d.object(last).expect("read").expect("last").payload else {
        panic!("cut")
    };
    let id = if index == 0 {
        e.previous.expect("first")
    } else {
        last
    };
    let read = ferritecad_document::ExtrudeEditSource::read(&d).expect("snapshot");
    let saved = read
        .cut_features
        .into_iter()
        .find(|c| c.feature == id)
        .expect("selected")
        .saved
        .expect("editable");
    d.close().expect("close");
    saved
}

fn edit_command(source: &Path, dest: &Path, saved: &SavedCircularCut, cut: CircularCut) -> Command {
    let catalog = inspect(source);
    let request = source.with_extension("edit.json");
    write(
        &request,
        &json!({"request_version":1,"tool_curve_id":saved.tool_curve,
        "center_mm":cut.center_mm,"radius_mm":cut.radius_mm,"depth_mm":cut.extent.blind_depth_mm().expect("blind")}),
    );
    let mut c = cli();
    c.arg(EDIT)
        .arg(source)
        .arg("--feature")
        .arg(saved.feature.to_string())
        .arg("--expect-version")
        .arg(catalog["content_version"].as_str().expect("version"))
        .arg("--request")
        .arg(request)
        .arg("-o")
        .arg(dest)
        .arg("--json");
    c
}

pub(super) fn edit(
    source: &Path,
    dest: &Path,
    saved: &SavedCircularCut,
    cut: CircularCut,
    code: i32,
) -> Value {
    reply(
        edit_command(source, dest, saved, cut)
            .output()
            .expect("edit"),
        EDIT,
        code,
    )
}

pub(super) fn only_numbers(source: &Path, copy: &Path, saved: &SavedCircularCut, added: usize) {
    let a = tables(source);
    let b = tables(copy);
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    for (name, (columns, rows)) in &a {
        let (cols, now) = &b[name];
        assert_eq!(columns, cols);
        if name == "topology_refs" {
            assert_eq!(now.len(), rows.len() + added);
            for row in rows {
                assert!(now.contains(row), "saved ref or rowid changed");
            }
            continue;
        }
        assert_eq!(rows.len(), now.len(), "{name} rows");
        for (old, new) in rows.iter().zip(now) {
            for (i, (x, y)) in old.iter().zip(new).enumerate() {
                if x == y {
                    continue;
                }
                let allowed = match name.as_str() {
                    "meta" => columns[i] == "modified_at",
                    "objects" => {
                        let id = columns.iter().position(|c| c == "id").expect("id column");
                        [saved.feature, saved.tool_sketch]
                            .iter()
                            .any(|v| old[id] == rusqlite::types::Value::Blob(v.to_bytes().to_vec()))
                            && matches!(columns[i].as_str(), "payload" | "payload_hash")
                    }
                    _ => false,
                };
                assert!(
                    allowed,
                    "unexpected cell {name}.{}: {x:?} -> {y:?}",
                    columns[i]
                );
            }
        }
    }
    let a = Document::open_read_only(source).expect("source");
    let b = Document::open_read_only(copy).expect("copy");
    assert_eq!(a.meta().document_id, b.meta().document_id);
    assert_eq!(
        a.dependencies().expect("deps"),
        b.dependencies().expect("deps")
    );
    assert_eq!(tip(&a), tip(&b));
    for old in a.objects().expect("objects") {
        let new = b.object(old.id).expect("object").expect("same UUID");
        match (&old.payload, &new.payload) {
            (ObjectPayload::Extrude(x), ObjectPayload::Extrude(y)) => {
                assert_eq!(x.previous, y.previous);
                assert_eq!(x.profile, y.profile);
            }
            (ObjectPayload::Sketch(x), ObjectPayload::Sketch(y)) => {
                assert_eq!(
                    x.curves.iter().map(|c| c.id).collect::<Vec<_>>(),
                    y.curves.iter().map(|c| c.id).collect::<Vec<_>>()
                );
            }
            _ => {}
        }
    }
}

#[test]
fn native_either_cut_edits_all_numbers_in_all_four_pairs() {
    if !native() {
        return;
    }
    use CacheOutcome::{Hit, Miss};
    for [d1, d2] in [[12., 12.], [12., 7.], [4., 12.], [4., 7.]] {
        let root = tempfile::tempdir().expect("root");
        let mut tools = TOOLS;
        tools[0].extent = CutExtent::Blind { depth_mm: d1 };
        tools[1].extent = CutExtent::Blind { depth_mm: d2 };
        let source = two(root.path(), tools);
        let original = std::fs::read(&source).expect("bytes");
        measure(&source, tools, Some([Miss; 3]));
        measure(&source, tools, Some([Hit; 3]));
        for index in 0..2 {
            let saved = selected(&source, index);
            for mode in 0..4 {
                let mut changed = tools;
                if mode == 0 || mode == 3 {
                    changed[index].center_mm[0] += 2.125;
                    changed[index].center_mm[1] -= 1.25;
                }
                if mode == 1 || mode == 3 {
                    changed[index].radius_mm += 0.625;
                }
                if mode == 2 || mode == 3 {
                    changed[index].extent = CutExtent::Blind {
                        depth_mm: if index == 0 { 3.25 } else { 5.5 },
                    };
                }
                let copy = root.path().join(format!("edit-{index}-{mode}.fcad"));
                let result = edit(&source, &copy, &saved, changed[index], 0);
                assert_eq!(
                    result["result"]["previous_feature_id"],
                    json!(saved.previous_feature)
                );
                assert_eq!(result["result"]["feature_id"], json!(saved.feature));
                let adds = if tools[index].extent.blind_depth_mm().expect("blind") == 12.
                    && changed[index].extent.blind_depth_mm().expect("blind") < 12.
                {
                    if index == 0 { 2 } else { 1 }
                } else {
                    0
                };
                only_numbers(&source, &copy, &saved, adds);
                let volume = measure(&copy, changed, None);
                // Transfer the actual old sidecar to the identity-preserving
                // copy, so stale entries are present and invalidation is tested.
                std::fs::copy(
                    source.with_extension("fcad-cache"),
                    copy.with_extension("fcad-cache"),
                )
                .expect("old cache");
                measure(
                    &copy,
                    changed,
                    Some(if index == 0 {
                        [Hit, Miss, Miss]
                    } else {
                        [Hit, Hit, Miss]
                    }),
                );
                measure(&copy, changed, Some([Hit; 3]));
                let fresh = root.path().join(format!("fresh-{index}-{mode}.fcad"));
                std::fs::copy(&copy, &fresh).expect("fresh sidecar fixture");
                measure(&fresh, changed, Some([Miss; 3]));
                measure(&fresh, changed, Some([Hit; 3]));
                check_two_mesh(&mesh(&copy, &copy.with_extension("stl")), changed);
                if mode == 3 && d1 == 12. {
                    let fbx = copy.with_extension("fbx");
                    reply(
                        cli()
                            .arg("export-fbx")
                            .arg(&copy)
                            .arg("-o")
                            .arg(&fbx)
                            .arg("--json")
                            .output()
                            .expect("fbx"),
                        "export-fbx",
                        0,
                    );
                    if let Some(dir) = std::env::var_os("FCAD_EDIT_SEQUENTIAL_ARTIFACTS") {
                        std::fs::create_dir_all(&dir).expect("artifacts");
                        std::fs::copy(fbx, Path::new(&dir).join(format!("edit-{index}-{d2}.fbx")))
                            .expect("keep");
                    }
                }
                if let Some(dir) = std::env::var_os("FCAD_EDIT_SEQUENTIAL_ARTIFACTS") {
                    use std::io::Write;
                    std::fs::create_dir_all(&dir).expect("artifact directory");
                    let mut log = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(Path::new(&dir).join("measurements.tsv"))
                        .expect("measurements");
                    writeln!(log, "{d1}/{d2}\t{index}\t{mode}\t{volume:.9}\t{adds}")
                        .expect("measure");
                }
                let again = root.path().join(format!("again-{index}-{mode}.fcad"));
                edit(&copy, &again, &selected(&copy, index), changed[index], 0);
                only_numbers(&copy, &again, &saved, 0);
                assert_eq!(std::fs::read(&source).expect("bytes"), original);
            }
        }
    }
}

#[test]
fn discovery_and_protected_floors_need_no_kernel() {
    let root = tempfile::tempdir().expect("root");
    let source = two(root.path(), TOOLS);
    let catalog = inspect(&source);
    assert_eq!(
        catalog["features"]
            .as_array()
            .expect("features")
            .iter()
            .filter(|f| f["circular_cut_edit"]["available"] == true)
            .count(),
        2
    );
    let before = std::fs::read(&source).expect("bytes");
    for (index, tool) in TOOLS.iter().enumerate() {
        let s = selected(&source, index);
        let d = Document::open_read_only(&source).expect("doc");
        let error = ferritecad_document::prepare_cut_parameters(
            &d,
            s.feature,
            &CircularCutEdit {
                tool_curve: s.tool_curve,
                center_mm: s.center_mm,
                radius_mm: s.radius_mm,
                extent: CutExtent::Blind { depth_mm: 12. },
                vocabulary: ExtentVocabulary::BlindOrThroughAll,
            },
        )
        .expect_err("protected floor before kernel");
        assert_eq!(
            s.protected_floor_references.len(),
            if index == 0 { 2 } else { 1 }
        );
        for id in s.protected_floor_references {
            assert!(error.to_string().contains(&id.to_string()));
        }
        if !ferritecad_occt::is_available() {
            let never = root.path().join("never.fcad");
            let result = edit(&source, &never, &selected(&source, index), *tool, 2);
            assert_eq!(result["error"]["kind"], "unsupported");
            assert!(!never.exists());
        }
    }
    // A valid replacement may not conceal an unsupported stored frame.
    let first = selected(&source, 0);
    let second = selected(&source, 1);
    for mode in 0..7 {
        let path = root.path().join(format!("unsupported-{mode}.fcad"));
        std::fs::copy(&source, &path).expect("fixture");
        let mut d = Document::open(&path).expect("doc");
        if mode < 3 {
            use ferritecad_document::{
                SketchConstraint, SketchConstraintRule, SketchPointRef, SketchPointSelector,
            };
            let id = [first.profile_sketch, first.tool_sketch, second.tool_sketch][mode];
            let mut record = d.object(id).expect("read").expect("sketch");
            let ObjectPayload::Sketch(sketch) = &mut record.payload else {
                panic!("sketch")
            };
            let selector = if mode == 0 {
                SketchPointSelector::Start
            } else {
                SketchPointSelector::Center
            };
            sketch.constraints.push(SketchConstraint {
                id: ferritecad_types::StableEntityId::new(),
                rule: SketchConstraintRule::Fixed {
                    point: SketchPointRef::new(sketch.curves[0].id, selector),
                    x: 0.,
                    y: 0.,
                },
            });
            d.write(|w| {
                w.put_object(
                    record.id,
                    record.parent,
                    record.ordinal,
                    record.name.as_deref(),
                    &record.payload,
                )
            })
            .expect("constraint fixture");
        } else if mode == 3 {
            d.write(|w| {
                w.add_dependency(ferritecad_document::Dependency {
                    dependent: second.feature,
                    dependency: first.tool_sketch,
                    role: ferritecad_document::DependencyRole::Parameter,
                })
            })
            .expect("extra edge");
        } else {
            let mut record = d.object(first.feature).expect("read").expect("feature");
            let ObjectPayload::Extrude(e) = &mut record.payload else {
                panic!("extrude")
            };
            match mode {
                4 => e.reversed = true,
                5 => e.operation = ferritecad_document::SolidOperation::Add,
                _ => e.end_condition = EndCondition::ThroughAll,
            };
            d.write(|w| {
                w.put_object(
                    record.id,
                    record.parent,
                    record.ordinal,
                    record.name.as_deref(),
                    &record.payload,
                )
            })
            .expect("unsupported feature");
        }
        let snapshot = ferritecad_document::ExtrudeEditSource::read(&d).expect("snapshot");
        assert!(
            snapshot.cut_features.iter().all(|c| c.saved.is_none()),
            "unsupported history offered: {mode}"
        );
        for saved in [&first, &second] {
            assert!(
                ferritecad_document::prepare_cut_parameters(
                    &d,
                    saved.feature,
                    &CircularCutEdit {
                        tool_curve: saved.tool_curve,
                        center_mm: saved.center_mm,
                        radius_mm: saved.radius_mm,
                        extent: CutExtent::Blind { depth_mm: 3. },
                        vocabulary: ExtentVocabulary::BlindOrThroughAll,
                    }
                )
                .is_err()
            );
        }
    }
    assert_eq!(std::fs::read(source).expect("bytes"), before);
}

#[test]
fn native_edit_refusals_and_late_guards_are_atomic() {
    history_refusals(2);
}

pub(super) fn history_refusals(count: usize) {
    if !native() {
        return;
    }
    for index in if count == 2 {
        vec![0, 1]
    } else {
        vec![0, count / 2, count - 1]
    } {
        let tools = if count == 2 {
            TOOLS.to_vec()
        } else {
            super::history::tools(count)
                .into_iter()
                .map(|t| CircularCut {
                    extent: CutExtent::Blind { depth_mm: 4. },
                    ..t
                })
                .collect()
        };
        let root = tempfile::tempdir().expect("root");
        let source = super::history::fixture(root.path(), &tools);
        let saved = super::history::ordered(&source)[index].clone();
        let never = root.path().join("never.fcad");
        let original = std::fs::read(&source).expect("source");
        for cut in [
            CircularCut {
                extent: CutExtent::Blind { depth_mm: 12. },
                ..tools[index]
            },
            CircularCut {
                center_mm: tools[(index + count - 1) % count].center_mm,
                ..tools[index]
            },
            CircularCut {
                center_mm: [0., 0.],
                ..tools[index]
            },
        ] {
            let error = edit(&source, &never, &saved, cut, 2);
            if cut.extent.blind_depth_mm().expect("blind") == 12. {
                for id in &saved.protected_floor_references {
                    assert!(error.to_string().contains(&id.to_string()));
                }
            }
            assert!(!never.exists());
        }
        for gap in [0., 0.5e-7, -1., if count == 2 { -8. } else { -2. }] {
            let other = tools[(index + count - 1) % count];
            let side = if index == 0 { -1. } else { 1. };
            let cut = CircularCut {
                center_mm: [
                    other.center_mm[0] + side * (other.radius_mm + saved.radius_mm + gap),
                    other.center_mm[1],
                ],
                ..tools[index]
            };
            assert!(
                edit(&source, &never, &saved, cut, 2)
                    .to_string()
                    .contains("separate")
            );
            assert!(!never.exists());
        }
        let delivered = root.path().join("lost-report.fcad");
        let mut changed = tools.clone();
        changed[index].extent = CutExtent::Blind { depth_mm: 3. };
        assert_eq!(
            edit_command(&source, &delivered, &saved, changed[index])
                .stdout(pipe::closed_pipe())
                .stderr(pipe::closed_pipe())
                .status()
                .expect("closed reports")
                .code(),
            Some(7)
        );
        measure_history(&delivered, &changed, None);
        assert_eq!(std::fs::read(&source).expect("source"), original);
        let mut wrong = saved.clone();
        wrong.tool_curve = ferritecad_types::StableEntityId::new();
        edit(&source, &never, &wrong, tools[index], 2);
        wrong = saved.clone();
        wrong.feature = ObjectId::new();
        edit(&source, &never, &wrong, tools[index], 2);
        edit(&source, &source, &saved, tools[index], 2);
        let alias = root.path().join("alias.fcad");
        std::fs::hard_link(&source, &alias).expect("alias");
        edit(&source, &alias, &saved, tools[index], 2);
        #[cfg(unix)]
        {
            let link = root.path().join("link.fcad");
            std::os::unix::fs::symlink(&source, &link).expect("link");
            edit(&source, &link, &saved, tools[index], 2);
        }
        std::fs::write(&never, b"occupied").expect("occupied");
        edit(&source, &never, &saved, tools[index], 2);
        assert_eq!(std::fs::read(&never).expect("bytes"), b"occupied");
        std::fs::remove_file(&never).expect("remove owned fixture");
        let d = Document::open_read_only(&source).expect("doc");
        let expected = ferritecad_document::DocumentVersion {
            document_id: d.meta().document_id,
            content: d.content_version().expect("version"),
        };
        d.close().expect("close");
        let request = EditCircularCutRequest {
            source: source.clone(),
            destination: never.clone(),
            expected,
            cut: saved.feature,
            edit: CircularCutEdit {
                tool_curve: saved.tool_curve,
                center_mm: saved.center_mm,
                radius_mm: saved.radius_mm,
                extent: CutExtent::Blind { depth_mm: 3. },
                vocabulary: ExtentVocabulary::BlindOrThroughAll,
            },
        };
        copy_late_guards(
            root.path(),
            &source,
            &original,
            &never,
            |kernel, context| edit_circular_cut_copy(&request, kernel, context).map(|_| ()),
        );
        // A stale CLI request is a separate early-guard assertion.
        let command = edit_command(&source, &never, &saved, tools[index]);
        let mut args = command
            .get_args()
            .map(|s| s.to_os_string())
            .collect::<Vec<_>>();
        let at = args
            .iter()
            .position(|s| s == "--expect-version")
            .expect("version arg");
        args[at + 1] = expected.content.to_string().into();
        let report = reply(cli().args(args).output().expect("stale CLI"), EDIT, 2);
        assert!(report.to_string().contains("source has changed"));
    }
}

#[test]
#[cfg(not(feature = "planegcs"))]
fn occt_without_solver_edits_both_history_links() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = two(root.path(), TOOLS);
    for index in 0..2 {
        let mut changed = TOOLS;
        changed[index].extent = CutExtent::Blind { depth_mm: 3. };
        let copy = root.path().join(format!("mixed-{index}.fcad"));
        edit(&source, &copy, &selected(&source, index), changed[index], 0);
        measure(&copy, changed, None);
    }
}

/// Exercise the actual shared copier at each late phase with the caller's geometry.
pub(super) fn copy_late_guards(
    root: &Path,
    source: &Path,
    original: &[u8],
    never: &Path,
    mut run: impl FnMut(
        &mut ferritecad_occt::OcctKernel,
        &OperationContext,
    ) -> ferritecad_types::Result<()>,
) {
    use ferritecad_kernel::{CancelToken, ProgressSink};
    let files = entries(root);
    for progress in [0.1, 0.4, 0.95] {
        let cancel = CancelToken::new();
        let signal = cancel.clone();
        let reached = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mark = reached.clone();
        let context = OperationContext::default()
            .with_cancel(cancel)
            .with_progress(ProgressSink::new(move |p| {
                if p >= progress {
                    mark.store(true, std::sync::atomic::Ordering::SeqCst);
                    signal.cancel();
                }
            }));
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        assert_eq!(
            run(&mut kernel, &context).expect_err("cancel").kind(),
            ferritecad_types::ErrorKind::Cancellation
        );
        assert!(reached.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(kernel.live_shape_count(), 0);
        assert_eq!(entries(root), files);
        assert_eq!(std::fs::read(source).expect("bytes"), original);
    }
    let path = source.to_path_buf();
    let reached = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mark = reached.clone();
    let context = OperationContext::default().with_progress(ProgressSink::new(move |p| {
        if p >= 0.95 && !mark.swap(true, std::sync::atomic::Ordering::SeqCst) {
            rusqlite::Connection::open(&path)
                .expect("SQL")
                .execute(
                    "UPDATE objects SET name='late change' WHERE kind='body'",
                    [],
                )
                .expect("change");
        }
    }));
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    assert!(
        run(&mut kernel, &context)
            .expect_err("late version")
            .to_string()
            .contains("source has changed")
    );
    assert!(reached.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(kernel.live_shape_count(), 0);
    assert_eq!(entries(root), files);
    assert!(!never.exists());
}
