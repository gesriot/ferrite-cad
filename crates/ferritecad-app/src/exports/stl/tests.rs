// SPDX-License-Identifier: MIT
//! Real peer CLI versus the form/request/owned-worker path used by the window.
#![allow(clippy::panic)]
use super::*;
use ferritecad_document::{
    Body, Dependency, DependencyRole, Document, EndCondition, Expression, ObjectPayload,
};
use ferritecad_kernel::ProgressSink;
use std::process::{Command, Output};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

fn native() -> bool {
    if !ferritecad_occt::is_available() {
        assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
        eprintln!("skipped: this build has no Open CASCADE (STL export gate)");
        return false;
    }
    if ferritecad_sketch_solver::is_required() {
        assert!(ferritecad_sketch_solver::is_available());
    }
    true
}
fn cli() -> Command {
    let mut path = std::env::current_exe().expect("binary");
    path.pop();
    path.pop();
    path.push(format!("ferritecad{}", std::env::consts::EXE_SUFFIX));
    Command::new(path)
}
fn success(command: &mut Command) -> Output {
    let output = command.output().expect("real CLI");
    assert!(output.status.success(), "{output:?}");
    output
}
fn make(path: &Path, size: [&str; 3]) {
    success(
        cli()
            .arg("create")
            .arg(path)
            .args(["--sample", "--size"])
            .args(size),
    );
}
fn scene(path: &Path) -> crate::LiveScene<()> {
    let mut kernel = OcctKernel::new().expect("kernel");
    let loaded = ferritecad_scene::snapshot_of(
        path,
        &mut kernel,
        |k, b| k.import_step(b),
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("accepted document");
    crate::LiveScene::new(
        Some(path.to_path_buf()),
        (),
        loaded.catalogue,
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Vec::new(),
    )
}
fn form(source: &Path) -> (Exports, ViewportInput, StlIntent) {
    let scene = scene(source);
    let mut exports = Exports::default();
    let mut input = ViewportInput::new();
    assert!(crate::can_export_stl(&scene));
    assert!(exports.ask_stl(
        scene.document.as_deref().expect("accepted path"),
        crate::stl_bodies(&scene),
        &mut input
    ));
    let intent = exports.stl_intent().expect("one body");
    (exports, input, intent)
}
fn peer(source: &Path, output: &Path, body: ObjectId, params: TessellationParams) -> Output {
    success(
        cli()
            .arg("export-stl")
            .arg(source)
            .arg("-o")
            .arg(output)
            .args([
                "--solid",
                &body.to_string(),
                "--linear-deflection",
                &params.linear_deflection().to_string(),
                "--angular-deflection",
                &params.angular_deflection().to_string(),
            ]),
    )
}
fn start(
    exports: &mut Exports,
    input: &mut ViewportInput,
    intent: &StlIntent,
    output: &Path,
    progress: ProgressSink,
) -> mpsc::Receiver<(ExportGeneration, Result<StlExport>)> {
    let (send, receive) = mpsc::channel();
    let captured = intent.clone();
    begin_stl_export(
        exports,
        input,
        intent,
        Some(output.to_path_buf()),
        move |destination, replace, generation, cancel| {
            let destination = destination.to_path_buf();
            let context = OperationContext::default()
                .with_cancel(cancel.clone())
                .with_progress(progress);
            spawn_export(
                move || run_stl_export(&captured, &destination, replace, &context),
                move |result| {
                    send.send((generation, result)).expect("deliver");
                },
            )
        },
    )
    .expect("worker starts");
    receive
}
fn finish(
    exports: &mut Exports,
    input: &mut ViewportInput,
    receive: mpsc::Receiver<(ExportGeneration, Result<StlExport>)>,
) {
    let (generation, result) = receive
        .recv_timeout(Duration::from_secs(20))
        .expect("worker reply");
    assert!(result.is_ok(), "{result:?}");
    assert!(finish_stl_export(exports, input, generation, result));
    assert!(matches!(exports.status(), ExportStatus::WroteStl { .. }));
    assert!(exports.status().omissions().is_empty());
    exports.stop_all();
}
fn dimensions(bytes: &[u8]) -> [f32; 3] {
    assert_eq!(bytes.len(), 684);
    assert_eq!(
        u32::from_le_bytes(bytes[80..84].try_into().expect("count")),
        12
    );
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for triangle in bytes[84..].chunks_exact(50) {
        for v in 0..3 {
            for axis in 0..3 {
                let start = 12 + v * 12 + axis * 4;
                let n = f32::from_le_bytes(triangle[start..start + 4].try_into().expect("float"));
                min[axis] = min[axis].min(n);
                max[axis] = max[axis].max(n);
            }
        }
    }
    assert_eq!(min, [0.0; 3]);
    max
}
fn entries(root: &Path) -> Vec<PathBuf> {
    let mut names: Vec<_> = std::fs::read_dir(root)
        .expect("dir")
        .map(|e| e.expect("entry").path())
        .collect();
    names.sort();
    names
}
fn two_bodies(source: &Path) -> (ObjectId, ObjectId) {
    make(source, ["60", "40", "10"]);
    let mut doc = Document::open(source).expect("fixture");
    let objects = doc.objects().expect("objects");
    let first = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("body")
        .clone();
    let mut extrude = objects
        .iter()
        .find_map(|o| {
            if let ObjectPayload::Extrude(e) = &o.payload {
                Some(e.clone())
            } else {
                None
            }
        })
        .expect("extrude");
    extrude.end_condition = EndCondition::Blind {
        distance: Expression::constant(23.0).expect("height"),
    };
    let feature = ObjectId::new();
    let second = ObjectId::new();
    doc.write(|w| {
        w.put_object(
            first.id,
            first.parent,
            first.ordinal,
            Some("Twin"),
            &first.payload,
        )?;
        w.put_object(
            feature,
            None,
            4,
            Some("Second extrusion"),
            &ObjectPayload::Extrude(extrude.clone()),
        )?;
        w.add_dependency(Dependency {
            dependent: feature,
            dependency: extrude.profile,
            role: DependencyRole::Profile,
        })?;
        w.put_object(
            second,
            None,
            5,
            Some("Twin"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(feature),
            }),
        )?;
        w.add_dependency(Dependency {
            dependent: second,
            dependency: feature,
            role: DependencyRole::BodyTip,
        })?;
        Ok(())
    })
    .expect("two geometrically distinct bodies");
    doc.close().expect("close");
    (first.id, second)
}

#[test]
fn native_stl_ui_and_cli_choose_the_same_body_and_write_identical_bytes() {
    if !native() {
        return;
    }
    for (sizes, expected) in [
        (["60", "40", "10"], [60.0, 40.0, 10.0]),
        (["91", "53", "17"], [91.0, 53.0, 17.0]),
    ] {
        let root = tempfile::tempdir().expect("root");
        let source = root.path().join("Плита с пробелами.fcad");
        make(&source, sizes);
        let before = std::fs::read(&source).expect("source");
        let (mut exports, mut input, intent) = form(&source);
        assert_eq!(intent.params, TessellationParams::default());
        let out = root.path().join("window.stl");
        let receive = start(
            &mut exports,
            &mut input,
            &intent,
            &out,
            ProgressSink::silent(),
        );
        finish(&mut exports, &mut input, receive);
        let peer_path = root.path().join("cli.stl");
        let report = peer(&source, &peer_path, intent.body, intent.params);
        assert!(
            String::from_utf8(report.stdout)
                .expect("stdout")
                .contains("(12 triangles, 684 bytes) from Plate")
        );
        let bytes = std::fs::read(&out).expect("UI file");
        assert_eq!(bytes, std::fs::read(&peer_path).expect("CLI file"));
        assert_eq!(dimensions(&bytes), expected);
        assert_eq!(std::fs::read(&source).expect("source unchanged"), before);
        assert_eq!(entries(root.path()).len(), 3);
    }
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("twins.fcad");
    let (first, second) = two_bodies(&source);
    let accepted = scene(&source);
    let bodies = crate::stl_bodies(&accepted);
    assert_eq!(bodies.len(), 2);
    assert_eq!(bodies[0].name, bodies[1].name);
    assert_ne!(bodies[0].label(), bodies[1].label());
    for (id, height) in [(first, 10.0), (second, 23.0)] {
        let mut exports = Exports::default();
        let mut input = ViewportInput::new();
        exports.ask_stl(&source, bodies.clone(), &mut input);
        assert!(
            exports.stl_intent().is_none(),
            "must explicitly choose among several bodies"
        );
        let shown = &mut exports.stl_form.as_mut().expect("form").shown;
        shown.selected = Some(id);
        shown.linear_mm = "0.02".into();
        shown.angular_rad = "0.3".into();
        let intent = exports.stl_intent().expect("explicit UUID");
        let out = root.path().join(format!("ui-{id}.stl"));
        let other = root.path().join(format!("cli-{id}.stl"));
        let receive = start(
            &mut exports,
            &mut input,
            &intent,
            &out,
            ProgressSink::silent(),
        );
        finish(&mut exports, &mut input, receive);
        peer(&source, &other, id, intent.params);
        let bytes = std::fs::read(out).expect("UI bytes");
        assert_eq!(bytes, std::fs::read(other).expect("CLI bytes"));
        assert_eq!(dimensions(&bytes), [60.0, 40.0, height]);
    }
    // An ID-shaped name of the second body may not steal the first body's ID.
    let mut doc = Document::open(&source).expect("fixture");
    let object = doc.object(second).expect("read").expect("second");
    doc.write(|w| {
        w.put_object(
            second,
            object.parent,
            -10,
            Some(&first.to_string()),
            &object.payload,
        )
    })
    .expect("rename/reorder");
    doc.close().expect("close");
    let out = root.path().join("id-priority.stl");
    peer(&source, &out, first, TessellationParams::default());
    assert_eq!(
        dimensions(&std::fs::read(out).expect("bytes")),
        [60.0, 40.0, 10.0]
    );
    // The old accepted catalogue remains an explicit ID choice after reordering.
    let mut exports = Exports::default();
    let mut input = ViewportInput::new();
    exports.ask_stl(&source, bodies, &mut input);
    exports.stl_form.as_mut().expect("form").shown.selected = Some(second);
    let intent = exports.stl_intent().expect("same durable second ID");
    let out = root.path().join("after-rename.stl");
    let receive = start(
        &mut exports,
        &mut input,
        &intent,
        &out,
        ProgressSink::silent(),
    );
    finish(&mut exports, &mut input, receive);
    assert_eq!(
        dimensions(&std::fs::read(out).expect("bytes")),
        [60.0, 40.0, 23.0]
    );
    let mut doc = Document::open(&source).expect("fixture");
    doc.write(|w| w.put_object(second, object.parent, -10, None, &object.payload))
        .expect("unnamed body");
    doc.close().expect("close");
    let unnamed = crate::stl_bodies(&scene(&source));
    let row = unnamed.iter().find(|body| body.id == second).expect("body");
    assert_eq!(row.name, None);
    assert_eq!(row.label(), second.to_string());
    exports.ask_stl(&source, unnamed, &mut input);
    assert!(exports.stl_intent().is_none());
    exports.stl_form.as_mut().expect("form").shown.selected = Some(second);
    let intent = exports.stl_intent().expect("unnamed UUID");
    let out = root.path().join("unnamed-ui.stl");
    let other = root.path().join("unnamed-cli.stl");
    let receive = start(
        &mut exports,
        &mut input,
        &intent,
        &out,
        ProgressSink::silent(),
    );
    finish(&mut exports, &mut input, receive);
    let report = peer(&source, &other, second, intent.params);
    assert!(
        String::from_utf8(report.stdout)
            .expect("stdout")
            .contains(&format!("from {second}\n"))
    );
    assert_eq!(
        std::fs::read(out).expect("UI"),
        std::fs::read(other).expect("CLI")
    );
}

#[test]
fn native_stl_refusals_and_saved_source_changes_are_honest() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("twins.fcad");
    let (first, second) = two_bodies(&source);
    let output = root.path().join("keep.stl");
    std::fs::write(&output, b"valuable export").expect("output");
    let before = std::fs::read(&source).expect("source");
    for selector in [
        None,
        Some("Twin".to_owned()),
        Some(ObjectId::new().to_string()),
    ] {
        let mut command = cli();
        command
            .arg("export-stl")
            .arg(&source)
            .arg("-o")
            .arg(&output)
            .arg("--force");
        if let Some(wanted) = selector {
            command.arg("--solid").arg(wanted);
        }
        let failed = command.output().expect("CLI");
        assert_eq!(failed.status.code(), Some(2));
        assert_eq!(std::fs::read(&output).expect("output"), b"valuable export");
        assert_eq!(std::fs::read(&source).expect("source"), before);
    }
    let accepted = scene(&source);
    let mut exports = Exports::default();
    let mut input = ViewportInput::new();
    exports.ask_stl(&source, crate::stl_bodies(&accepted), &mut input);
    exports.stl_form.as_mut().expect("form").shown.selected = Some(first);
    exports.stl_form.as_mut().expect("form").shown.linear_mm = "NaN".into();
    assert!(exports.stl_intent().is_none());
    exports.stl_form.as_mut().expect("form").shown.linear_mm = "0.01".into();
    let intent = exports.stl_intent().expect("valid settings");
    for linear in ["0", "NaN", "inf"] {
        let failed = cli()
            .arg("export-stl")
            .arg(&source)
            .arg("-o")
            .arg(&output)
            .args([
                "--force",
                "--solid",
                &first.to_string(),
                "--linear-deflection",
                linear,
            ])
            .output()
            .expect("CLI");
        assert_eq!(failed.status.code(), Some(2));
        assert_eq!(std::fs::read(&output).expect("output"), b"valuable export");
    }
    // Aliases are refused before any worker, including after Replace confirmation.
    let hard = root.path().join("hard.fcad");
    std::fs::hard_link(&source, &hard).expect("hard link");
    let aliases = [
        source.clone(),
        hard,
        #[cfg(unix)]
        {
            let link = root.path().join("symbolic.fcad");
            std::os::unix::fs::symlink(&source, &link).expect("symlink");
            link
        },
    ];
    for alias in aliases {
        assert!(
            begin_stl_export(
                &mut exports,
                &mut input,
                &intent,
                Some(alias.clone()),
                |_, _, _, _| panic!("alias worker")
            )
            .is_none()
        );
        assert!(matches!(exports.status(), ExportStatus::Failed { .. }));
        let failed = cli()
            .arg("export-stl")
            .arg(&source)
            .arg("-o")
            .arg(&alias)
            .args(["--force", "--solid", &first.to_string()])
            .output()
            .expect("CLI");
        assert_eq!(failed.status.code(), Some(2));
        assert_eq!(std::fs::read(alias).expect("alias unchanged"), before);
    }
    // Cold reads the saved document: a height changed since Open is exported.
    let mut doc = Document::open(&source).expect("change");
    let body = doc.object(first).expect("read").expect("body");
    let ObjectPayload::Body(b) = body.payload else {
        unreachable!()
    };
    let mut feature = doc
        .object(b.tip_feature.expect("tip"))
        .expect("read")
        .expect("feature");
    let ObjectPayload::Extrude(ref mut e) = feature.payload else {
        unreachable!()
    };
    e.end_condition = EndCondition::Blind {
        distance: Expression::constant(31.0).expect("height"),
    };
    doc.write(|w| {
        w.put_object(
            feature.id,
            feature.parent,
            feature.ordinal,
            feature.name.as_deref(),
            &feature.payload,
        )
    })
    .expect("change since Open");
    doc.close().expect("close");
    let out = root.path().join("fresh-saved.stl");
    let receive = start(
        &mut exports,
        &mut input,
        &intent,
        &out,
        ProgressSink::silent(),
    );
    finish(&mut exports, &mut input, receive);
    assert_eq!(
        dimensions(&std::fs::read(out).expect("new height")),
        [60.0, 40.0, 31.0]
    );
    // Remove the selected Body while another remains; no fallback to that other ID.
    let sql = rusqlite::Connection::open(&source).expect("fixture");
    sql.execute("DELETE FROM deps WHERE dependent_id=?1", [first.to_bytes()])
        .expect("deps");
    sql.execute("DELETE FROM objects WHERE id=?1", [first.to_bytes()])
        .expect("body");
    drop(sql);
    let changed = std::fs::read(&source).expect("changed source");
    let result = run_stl_export(&intent, &output, true, &OperationContext::default())
        .expect_err("selected ID vanished");
    assert_eq!(result.kind(), ErrorKind::Input);
    assert!(result.to_string().contains(&first.to_string()));
    assert_eq!(std::fs::read(&output).expect("output"), b"valuable export");
    assert_eq!(std::fs::read(&source).expect("source"), changed);
    assert!(
        Document::open_read_only(&source)
            .expect("read")
            .object(second)
            .expect("read")
            .is_some()
    );
    // Read-only refusal precedes geometry and leaves the WAL mode/source alone.
    let wal = root.path().join("wal.fcad");
    make(&wal, ["60", "40", "10"]);
    let (_, _, wal_intent) = form(&wal);
    rusqlite::Connection::open(&wal)
        .expect("fixture")
        .execute_batch("PRAGMA journal_mode=WAL;")
        .expect("WAL");
    let before = std::fs::read(&wal).expect("WAL bytes");
    assert_eq!(
        run_stl_export(&wal_intent, &output, true, &OperationContext::default())
            .expect_err("WAL")
            .kind(),
        ErrorKind::Unsupported
    );
    assert_eq!(std::fs::read(wal).expect("WAL unchanged"), before);
    assert_eq!(
        std::fs::read(output).expect("output unchanged"),
        b"valuable export"
    );
}

fn barrier(fraction: f64) -> (ProgressSink, mpsc::Receiver<()>, mpsc::Sender<()>) {
    let (arrived, ready) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let wait = std::sync::Mutex::new(wait);
    (
        ProgressSink::new(move |value| {
            if value == fraction {
                arrived.send(()).expect("arrive");
                wait.lock()
                    .expect("wait")
                    .recv_timeout(Duration::from_secs(20))
                    .expect("release worker");
            }
        }),
        ready,
        release,
    )
}

#[test]
fn native_stl_owned_workers_keep_publication_and_stale_answers_honest() {
    if !native() {
        return;
    }
    for event in ["cancel", "racer", "late cancel"] {
        let root = tempfile::tempdir().expect("root");
        let source = root.path().join("source.fcad");
        make(&source, ["60", "40", "10"]);
        let before = std::fs::read(&source).expect("source");
        let (mut exports, mut input, intent) = form(&source);
        let output = root.path().join("out.stl");
        let (progress, ready, release) = barrier(if event == "late cancel" { 1.0 } else { 0.95 });
        let result = start(&mut exports, &mut input, &intent, &output, progress);
        ready
            .recv_timeout(Duration::from_secs(20))
            .expect("worker at publication seam");
        if event == "racer" {
            std::fs::write(&output, b"racing destination").expect("racer");
        } else {
            assert!(cancel_export(&mut exports, &mut input));
        }
        if event == "late cancel" {
            assert_eq!(
                std::fs::read(&output).expect("already published").len(),
                684
            );
        }
        release.send(()).expect("release");
        let (generation, result) = result.recv_timeout(Duration::from_secs(20)).expect("reply");
        assert_eq!(
            result.is_ok(),
            event == "late cancel",
            "{event}: {result:?}"
        );
        assert!(finish_stl_export(
            &mut exports,
            &mut input,
            generation,
            result
        ));
        exports.stop_all();
        match event {
            "cancel" => {
                assert!(matches!(exports.status(), ExportStatus::Cancelled { .. }));
                assert!(!output.exists());
            }
            "racer" => {
                assert!(matches!(exports.status(), ExportStatus::Failed { .. }));
                assert_eq!(
                    std::fs::read(&output).expect("racer"),
                    b"racing destination"
                );
                assert!(!exports.status().line().contains("--force"));
            }
            "late cancel" => {
                assert!(matches!(exports.status(), ExportStatus::WroteStl { .. }));
                assert_eq!(
                    dimensions(&std::fs::read(&output).expect("published")),
                    [60.0, 40.0, 10.0]
                );
            }
            _ => unreachable!(),
        }
        assert_eq!(std::fs::read(&source).expect("unchanged"), before);
        assert_eq!(
            entries(root.path()).len(),
            if event == "cancel" { 1 } else { 2 }
        );
    }
    for transition in ["new request", "new document"] {
        let root = tempfile::tempdir().expect("root");
        let source = root.path().join("source.fcad");
        make(&source, ["60", "40", "10"]);
        let (mut exports, mut input, intent) = form(&source);
        let old = root.path().join("old.stl");
        let (sent, ready) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let (answer, receive) = mpsc::channel();
        let captured = intent.clone();
        begin_stl_export(
            &mut exports,
            &mut input,
            &intent,
            Some(old.clone()),
            move |destination, replace, generation, cancel| {
                let destination = destination.to_path_buf();
                let context = OperationContext::default().with_cancel(cancel.clone());
                spawn_export(
                    move || {
                        let result = run_stl_export(&captured, &destination, replace, &context);
                        assert!(result.is_ok(), "{result:?}");
                        sent.send(()).expect("published");
                        held.recv_timeout(Duration::from_secs(20))
                            .expect("release old answer");
                        result
                    },
                    move |result| answer.send((generation, result)).expect("answer"),
                )
            },
        )
        .expect("old worker");
        ready
            .recv_timeout(Duration::from_secs(20))
            .expect("old publication");
        assert_eq!(std::fs::read(&old).expect("old file").len(), 684);
        let mut next = None;
        if transition == "new request" {
            let out = root.path().join("new.stl");
            let (progress, ready, release) = barrier(0.95);
            let result = start(&mut exports, &mut input, &intent, &out, progress);
            ready
                .recv_timeout(Duration::from_secs(20))
                .expect("new worker running");
            next = Some((result, release));
        } else {
            let other = root.path().join("other.fcad");
            make(&other, ["91", "53", "17"]);
            let scene = scene(&other);
            super::super::leave_document(&mut exports, &mut input);
            exports.ask_stl(&other, crate::stl_bodies(&scene), &mut input);
            assert_eq!(
                exports.stl_intent().expect("new document form").document,
                other
            );
        }
        let current = exports.status().clone();
        release.send(()).expect("deliver old");
        let (generation, result) = receive
            .recv_timeout(Duration::from_secs(20))
            .expect("old response");
        assert!(
            !finish_stl_export(&mut exports, &mut input, generation, result),
            "stale answer accepted after {transition}"
        );
        assert_eq!(*exports.status(), current);
        if let Some((result, release)) = next {
            release.send(()).expect("finish current");
            finish(&mut exports, &mut input, result);
        }
        exports.stop_all();
        assert!(
            old.exists(),
            "stale successful reply never retracts its publication"
        );
    }
    // Shutdown owns the running thread until it releases the native session.
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("source.fcad");
    make(&source, ["60", "40", "10"]);
    let (mut exports, mut input, intent) = form(&source);
    let output = root.path().join("shutdown.stl");
    let captured = intent.clone();
    let (at_seam, ready) = mpsc::channel();
    let (answer, result) = mpsc::channel();
    let ended = Arc::new(AtomicBool::new(false));
    let finished = ended.clone();
    begin_stl_export(
        &mut exports,
        &mut input,
        &intent,
        Some(output.clone()),
        move |destination, replace, _, cancel| {
            let destination = destination.to_path_buf();
            let stop = cancel.clone();
            let context = OperationContext::default()
                .with_cancel(cancel.clone())
                .with_progress(ProgressSink::new(move |fraction| {
                    if fraction == 0.95 {
                        at_seam.send(()).expect("ready");
                        let deadline = std::time::Instant::now() + Duration::from_secs(10);
                        while !stop.is_cancelled() && std::time::Instant::now() < deadline {
                            std::thread::yield_now();
                        }
                        assert!(stop.is_cancelled(), "shutdown failed to cancel");
                    }
                }));
            spawn_export(
                move || run_stl_export(&captured, &destination, replace, &context),
                move |outcome| {
                    answer.send(outcome).expect("reply");
                    finished.store(true, Ordering::SeqCst);
                },
            )
        },
    )
    .expect("start shutdown worker");
    ready
        .recv_timeout(Duration::from_secs(20))
        .expect("worker held");
    assert!(!ended.load(Ordering::SeqCst));
    exports.stop_all();
    assert!(
        ended.load(Ordering::SeqCst),
        "worker joined before shutdown returns"
    );
    assert_eq!(
        result
            .recv()
            .expect("outcome")
            .expect_err("cancelled")
            .kind(),
        ErrorKind::Cancellation
    );
    assert!(!output.exists());
    assert_eq!(entries(root.path()), vec![source]);
}

#[test]
fn native_stl_dialog_choices_and_unavailable_documents_do_no_unrequested_work() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("source.fcad");
    make(&source, ["60", "40", "10"]);
    let before = std::fs::read(&source).expect("source");
    let (mut exports, mut input, intent) = form(&source);
    let status = exports.status().clone();
    assert!(
        begin_stl_export(
            &mut exports,
            &mut input,
            &intent,
            None,
            |_, _, _, _| panic!("closed dialog spawned")
        )
        .is_none()
    );
    assert_eq!(*exports.status(), status);
    assert!(exports.configuring_stl());
    assert_eq!(entries(root.path()), vec![source.clone()]);
    let output = root.path().join("existing.stl");
    std::fs::write(&output, b"older").expect("sentinel");
    begin_stl_export(
        &mut exports,
        &mut input,
        &intent,
        Some(output.clone()),
        |_, _, _, _| panic!("no confirmation"),
    );
    assert_eq!(exports.pending(), Some(output.as_path()));
    assert_eq!(
        exports.pending_stl().expect("captured request").body,
        intent.body
    );
    confirm_stl_export(
        &mut exports,
        &mut input,
        ReplaceChoice::Cancel,
        |_, _, _, _| panic!("declined replacement"),
    );
    assert!(exports.pending().is_none());
    assert_eq!(std::fs::read(&output).expect("older"), b"older");
    begin_stl_export(
        &mut exports,
        &mut input,
        &intent,
        Some(output.clone()),
        |_, _, _, _| panic!("no confirmation"),
    );
    let (send, receive) = mpsc::channel();
    let captured = intent.clone();
    confirm_stl_export(
        &mut exports,
        &mut input,
        ReplaceChoice::Replace,
        move |destination, replace, generation, cancel| {
            assert!(replace);
            let destination = destination.to_path_buf();
            let context = OperationContext::default().with_cancel(cancel.clone());
            spawn_export(
                move || run_stl_export(&captured, &destination, replace, &context),
                move |result| send.send((generation, result)).expect("reply"),
            )
        },
    )
    .expect("confirmed replacement");
    finish(&mut exports, &mut input, receive);
    assert_eq!(
        std::fs::read(&output).expect("whole replacement").len(),
        684
    );
    // Confirmation cannot authorize an alias that appeared after the question.
    begin_stl_export(
        &mut exports,
        &mut input,
        &intent,
        Some(output.clone()),
        |_, _, _, _| panic!("no confirmation"),
    );
    std::fs::remove_file(&output).expect("remove own STL");
    std::fs::hard_link(&source, &output).expect("alias since dialog");
    confirm_stl_export(
        &mut exports,
        &mut input,
        ReplaceChoice::Replace,
        |_, _, _, _| panic!("Replace bypassed source guard"),
    );
    assert!(matches!(exports.status(), ExportStatus::Failed { .. }));
    assert_eq!(std::fs::read(&source).expect("source"), before);
    assert_eq!(std::fs::read(&output).expect("alias"), before);
    let empty = root.path().join("empty.fcad");
    success(cli().arg("create").arg(&empty));
    let imported = root.path().join("imported.fcad");
    let step = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step/canonical/01-single-part.step");
    success(cli().arg("import-step").arg(step).arg("-o").arg(&imported));
    for source in [empty, imported] {
        let mut accepted = scene(&source);
        assert!(!crate::can_export_stl(&accepted));
        assert!(crate::stl_bodies(&accepted).is_empty());
        assert!(!exports.ask_stl(&source, crate::stl_bodies(&accepted), &mut input));
        let before = std::fs::read(&source).expect("source");
        let refused = root.path().join("refused.stl");
        let failed = cli()
            .arg("export-stl")
            .arg(&source)
            .arg("-o")
            .arg(&refused)
            .output()
            .expect("real CLI");
        assert_eq!(failed.status.code(), Some(2));
        assert!(
            String::from_utf8(failed.stderr)
                .expect("stderr")
                .contains("no bodies")
        );
        assert!(!refused.exists());
        assert_eq!(std::fs::read(source).expect("source unchanged"), before);
        accepted.document = None;
        assert!(!crate::can_export_stl(&accepted));
        assert!(crate::stl_bodies(&accepted).is_empty());
    }
}

#[test]
fn no_native_stl_refuses_geometry_without_touching_files() {
    if ferritecad_occt::is_available() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("source.fcad");
    make(&source, ["60", "40", "10"]);
    let before = std::fs::read(&source).expect("source");
    let doc = Document::open_read_only(&source).expect("read");
    let body = doc
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("body")
        .id;
    doc.close().expect("close");
    let intent = StlIntent {
        document: source.clone(),
        body,
        params: TessellationParams::default(),
    };
    let output = root.path().join("keep.stl");
    std::fs::write(&output, b"keep").expect("sentinel");
    assert_eq!(
        run_stl_export(&intent, &output, true, &OperationContext::default())
            .expect_err("no OCCT")
            .kind(),
        ErrorKind::Unsupported
    );
    let failed = cli()
        .arg("export-stl")
        .arg(&source)
        .arg("-o")
        .arg(&output)
        .arg("--force")
        .output()
        .expect("CLI");
    assert_eq!(failed.status.code(), Some(2));
    assert!(
        String::from_utf8(failed.stderr)
            .expect("stderr")
            .contains("no Open CASCADE")
    );
    assert_eq!(std::fs::read(&source).expect("source unchanged"), before);
    assert_eq!(std::fs::read(&output).expect("output unchanged"), b"keep");
    assert_eq!(entries(root.path()).len(), 2);
}
