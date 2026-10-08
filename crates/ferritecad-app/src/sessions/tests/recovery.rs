// SPDX-License-Identifier: MIT
//! §30M: the window's crash copies. The recorder is told at the one place a
//! version is accepted with its picture, saved or replaced (`Sessions`), and a
//! recovered copy comes back through the same claim, restore and `Bind::Open` the
//! window uses. The crash is real: the test binary starts itself as a child
//! process that edits through the window's own workers, waits until the copy is
//! confirmed written, and is killed by its own PID.

use super::fillet::{saved, sql};
use super::*;
use ferritecad_jobs::{RecordId, RecoveryRecorder, RecoveryStatus, RecoveryStore, SaveTarget};
use std::io::BufRead as _;
use std::time::{Duration, Instant};

fn recording(store: &RecoveryStore) -> Sessions {
    let mut sessions = Sessions::default();
    sessions.keep_recovery(RecoveryRecorder::start(store.clone(), || {}));
    sessions
}

fn settle(sessions: &Sessions) -> Option<RecoveryStatus> {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !sessions.recovery_settled() {
        assert!(Instant::now() < deadline, "the recorder never settled");
        std::thread::sleep(Duration::from_millis(10));
    }
    sessions.recovery_status()
}

fn written(sessions: &Sessions) {
    match settle(sessions) {
        Some(RecoveryStatus::Written { .. }) => {}
        other => panic!("not written: {other:?}"),
    }
}

/// Every copy any record of the folder holds now, held by a window or not.
fn copies(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for record in std::fs::read_dir(root).expect("root") {
        let record = record.expect("entry").path();
        if !record.is_dir() || !record.join("manifest").exists() {
            continue;
        }
        for file in std::fs::read_dir(&record).expect("record") {
            let file = file.expect("entry").path();
            let name = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if name.starts_with('c') && name.ends_with(".fcad") {
                found.push(file);
            }
        }
    }
    found
}

fn records(root: &Path) -> usize {
    std::fs::read_dir(root)
        .expect("root")
        .filter(|e| {
            e.as_ref()
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with("r-")
        })
        .count()
}

fn content(path: &Path) -> ferritecad_types::ContentHash {
    let document = Document::open_read_only(path).expect("document");
    let content = document.content_version().expect("content");
    document.close().expect("close");
    content
}

#[test]
fn only_a_version_shown_with_its_picture_is_copied_and_saved_or_undone_states_have_no_copy() {
    let f = fixture();
    let root = tempfile::tempdir().expect("recovery");
    let store = RecoveryStore::open(root.path()).expect("store");
    let mut s = recording(&store);
    s.adopt(
        DocumentSession::open_in(f.private.path(), &f.file, HistoryLimits::default())
            .expect("session"),
    );
    assert_eq!(settle(&s), Some(RecoveryStatus::Off), "a clean Open");
    assert!(copies(root.path()).is_empty());

    // Produced, but its picture failed: never copied.
    let (generation, edited) = apply(&mut s, f.feature, 25.0);
    assert!(matches!(edited, Edited::Show(_)));
    assert!(s.finish_scene(generation, Err(CadError::rendering("no device"))));
    assert_eq!(settle(&s), Some(RecoveryStatus::Off));
    assert!(
        copies(root.path()).is_empty(),
        "an unshown version was copied"
    );

    // Produced, then cancelled before it was shown: never copied.
    let (generation, edited) = apply(&mut s, f.feature, 26.0);
    assert!(matches!(edited, Edited::Show(_)));
    assert!(s.cancel());
    let refused = s.bind(Bind::Staged);
    assert!(refused.is_err(), "a cancelled picture is not shown");
    assert!(s.finish_scene(generation, refused));
    assert_eq!(settle(&s), Some(RecoveryStatus::Off));
    assert!(
        copies(root.path()).is_empty(),
        "a cancelled candidate was copied"
    );
    assert!(!s.dirty());

    // Shown: copied, exactly the accepted version.
    let (generation, _) = apply(&mut s, f.feature, 27.0);
    s.bind(Bind::Staged).expect("shown");
    assert!(s.finish_scene(generation, Ok(())));
    written(&s);
    let accepted = s.export_path().expect("accepted");
    let kept = copies(root.path());
    assert_eq!(kept.len(), 1);
    assert_eq!(content(&kept[0]), content(&accepted));
    assert!(
        s.recovery_line()
            .is_some_and(|line| line.starts_with("Recovery copy written ")),
        "{:?}",
        s.recovery_line()
    );

    // Undo back to what is saved: nothing unsaved, no copy.
    let (generation, _) = s.begin_move(true).expect("undo");
    s.bind(Bind::Staged).expect("shown");
    assert!(s.finish_scene(generation, Ok(())));
    assert_eq!(settle(&s), Some(RecoveryStatus::Off));
    assert!(copies(root.path()).is_empty());
    assert_eq!(s.recovery_line(), None);

    // Redo: copied again.
    let (generation, _) = s.begin_move(false).expect("redo");
    s.bind(Bind::Staged).expect("shown");
    assert!(s.finish_scene(generation, Ok(())));
    written(&s);
    assert_eq!(copies(root.path()).len(), 1);

    // A published Save: no copy.
    saved(&mut s, SaveTarget::InPlace);
    assert_eq!(settle(&s), Some(RecoveryStatus::Off));
    assert!(copies(root.path()).is_empty());
    s.decide_exit();
    s.stop_all();
    assert_eq!(records(root.path()), 0);
}

#[test]
fn discard_and_quit_end_the_copy_and_an_exit_nobody_chose_keeps_it() {
    let f = fixture();
    let root = tempfile::tempdir().expect("recovery");
    let store = RecoveryStore::open(root.path()).expect("store");

    // Discard: the dirty document is replaced by one the person accepted.
    let mut s = recording(&store);
    s.adopt(
        DocumentSession::open_in(f.private.path(), &f.file, HistoryLimits::default())
            .expect("session"),
    );
    let (generation, _) = apply(&mut s, f.feature, 25.0);
    s.bind(Bind::Staged).expect("shown");
    assert!(s.finish_scene(generation, Ok(())));
    written(&s);
    assert_eq!(s.replacing(Some(UnsavedChoice::Cancel)), Replace::Stay);
    assert_eq!(copies(root.path()).len(), 1, "Cancel keeps the copy");
    assert_eq!(
        s.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    assert_eq!(copies(root.path()).len(), 1, "kept until the replacement");
    s.adopt(
        DocumentSession::open_in(f.private.path(), &f.file, HistoryLimits::default())
            .expect("the replacement"),
    );
    assert_eq!(settle(&s), Some(RecoveryStatus::Off));
    assert!(copies(root.path()).is_empty(), "Discard ended the copy");

    // An exit nobody decided: the dirty document's copy stays for next time.
    let (generation, _) = apply(&mut s, f.feature, 31.0);
    s.bind(Bind::Staged).expect("shown");
    assert!(s.finish_scene(generation, Ok(())));
    written(&s);
    let accepted = content(&s.export_path().expect("accepted"));
    s.stop_all();
    let found: Vec<_> = store.list().expect("list").recoverable().cloned().collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].content, accepted);
    assert_eq!(found[0].name, "plate.fcad");

    // Quit after the question: the copy goes.
    let mut s = recording(&store);
    s.adopt(
        DocumentSession::open_in(f.private.path(), &f.file, HistoryLimits::default())
            .expect("session"),
    );
    let (generation, _) = apply(&mut s, f.feature, 33.0);
    s.bind(Bind::Staged).expect("shown");
    assert!(s.finish_scene(generation, Ok(())));
    written(&s);
    s.decide_exit();
    s.stop_all();
    assert_eq!(
        store.list().expect("list").recoverable().count(),
        1,
        "only the copy nobody decided about is left"
    );
}

/// An orphaned record holding the fixture's plate after one height edit.
fn orphan(f: &Fixture, store: &RecoveryStore, millimetres: f64) -> (RecordId, DocumentSession) {
    let mut session = DocumentSession::open_in(f.private.path(), &f.file, HistoryLimits::default())
        .expect("session");
    let step = session
        .begin_step()
        .edit_extrude_height(
            f.feature,
            millimetres,
            &mut MockKernel::new(),
            &OperationContext::default(),
        )
        .expect("edit");
    session.commit_step(step).expect("accepted");
    let mut record = store.create_record().expect("record");
    record
        .publish(1, &session.current(), &session.recovery_name())
        .expect("copy");
    let id = record.id();
    drop(record);
    (id, session)
}

#[test]
fn a_recovered_document_is_untitled_named_recovered_and_takes_over_its_record() {
    let f = fixture();
    let root = tempfile::tempdir().expect("recovery");
    let store = RecoveryStore::open(root.path()).expect("store");
    let original = std::fs::read(&f.file).expect("source");
    let (id, before) = orphan(&f, &store, 21.5);

    // A candidate that is never accepted lets the record go, untouched.
    let claim = store.claim(id).expect("claim");
    let candidate = DocumentSession::recover_in(f.private.path(), HistoryLimits::default(), claim)
        .expect("candidate");
    drop(candidate);
    assert_eq!(store.list().expect("list").recoverable().count(), 1);

    let mut s = recording(&store);
    let claim = store.claim(id).expect("claim");
    let session = DocumentSession::recover_in(f.private.path(), HistoryLimits::default(), claim)
        .expect("recovered");
    // The window binds it exactly as it binds an Open.
    s.bind(Bind::Open(Box::new(session))).expect("bound");
    written(&s);
    assert_eq!(s.title(), "*plate.fcad (recovered) — FerriteCAD");
    assert!(s.untitled() && s.recovered() && s.dirty() && !s.can_undo());
    assert_eq!(s.logical_path(), None);
    assert_eq!(
        sql(&s.export_path().expect("current")),
        sql(before.current().path()),
        "every SQL cell, row id and reference as recorded"
    );
    assert_eq!(records(root.path()), 1, "adopted, not duplicated");
    assert_eq!(store.list().expect("list").active, 1, "held by this window");

    // The first Save is a Save As; the source is never written.
    let out = f.root.path().join("recovered.fcad");
    saved(&mut s, SaveTarget::As(out.clone()));
    assert_eq!(s.title(), "recovered.fcad — FerriteCAD");
    assert!(!s.recovered());
    assert_eq!(settle(&s), Some(RecoveryStatus::Off));
    assert!(copies(root.path()).is_empty());
    assert_eq!(std::fs::read(&f.file).expect("source"), original);
    s.decide_exit();
    s.stop_all();
    assert_eq!(records(root.path()), 0);
}

#[test]
fn stub_recovery_is_refused_at_the_picture_and_keeps_the_record_and_the_open_document() {
    if ferritecad_occt::is_available() {
        return;
    }
    let f = fixture();
    let root = tempfile::tempdir().expect("recovery");
    let store = RecoveryStore::open(root.path()).expect("store");
    let (id, before) = orphan(&f, &store, 21.5);
    let mut s = Sessions::default();
    s.adopt(
        DocumentSession::open_in(f.private.path(), &f.file, HistoryLimits::default())
            .expect("session"),
    );
    let refused = recover_for_view(f.private.path(), &store, id, &OperationContext::default())
        .map(|_| ())
        .expect_err("no kernel to draw it with");
    assert_eq!(refused.kind(), ErrorKind::Unsupported, "{refused}");
    let found: Vec<_> = store.list().expect("list").recoverable().cloned().collect();
    assert_eq!(found.len(), 1, "the record is untouched");
    assert_eq!(found[0].content, before.current().version().content);
    assert_eq!(
        s.title(),
        "plate.fcad — FerriteCAD",
        "the open document stays"
    );
    println!("FCAD_30M_STUB_RECOVERY_EXECUTED");
}

#[test]
fn mixed_recovery_draws_without_a_solver() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        return;
    }
    let f = fixture();
    let root = tempfile::tempdir().expect("recovery");
    let store = RecoveryStore::open(root.path()).expect("store");
    let (id, before) = orphan(&f, &store, 21.5);
    let (scene, session) =
        recover_for_view(f.private.path(), &store, id, &OperationContext::default())
            .expect("recovered with OCCT and no solver");
    assert!(!scene.catalogue.is_empty());
    assert_eq!(session.current().version(), before.current().version());
    println!("FCAD_30M_MIXED_RECOVERY_EXECUTED");
}

// ---- the native crash matrix -------------------------------------------------

const CHILD: &str = "sessions::tests::recovery::recovery_child_entry";
const MODE: &str = "FCAD_RECOVERY_APP_CHILD_MODE";
const ROOT: &str = "FCAD_RECOVERY_APP_CHILD_ROOT";
const WORK: &str = "FCAD_RECOVERY_APP_CHILD_WORK";
const SAY: &str = "FCAD-RECOVERY-CHILD ";

fn plate_file(path: &Path) {
    create_document(
        CreateDocumentRequest::new(
            path,
            NewDocument::SamplePlate(PlateSize {
                width: 80.0,
                depth: 40.0,
                height: 12.0,
            }),
            "keep",
        ),
        &OperationContext::default(),
    )
    .expect("a plate");
}

/// The window's own creation, bound as the window binds it.
fn create_native(s: &mut Sessions, root: &Path, content: NewDocument) {
    let (_scene, session) =
        create_for_view(root, content, &OperationContext::default()).expect("created");
    s.bind(Bind::Open(Box::new(session))).expect("bound");
}

/// The child: edits through the window's own workers, waits for the copy, hangs.
#[test]
#[ignore = "run by the native crash matrix as a child process"]
fn recovery_child_entry() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    let root = PathBuf::from(std::env::var_os(ROOT).expect("root"));
    let work = PathBuf::from(std::env::var_os(WORK).expect("work"));
    let store = RecoveryStore::open(&root).expect("store");
    let private = work.join("private");
    std::fs::create_dir_all(&private).expect("private");
    let mut s = recording(&store);
    match mode.as_str() {
        "plate" => {
            let file = work.join("plate.fcad");
            s.adopt(
                DocumentSession::open_in(&private, &file, HistoryLimits::default()).expect("open"),
            );
            let opened = read_extrude_source(&file).expect("reading");
            let feature = opened.features[0].feature;
            // Height (OCCT), constraints (PlaneGCS), Add Fillet (OCCT), Undo, Redo.
            apply_native(&mut s, feature, 21.5);
            let current = s.export_path().expect("current");
            let reading = read_extrude_source(&current).expect("reading");
            let choice = &reading.constraint_sketches[0];
            let curve = choice.stored.as_ref().expect("stored").curves[0].id;
            apply_constraints_native(
                &mut s,
                &ferritecad_jobs::EditSketchConstraintsRequest {
                    source: current.clone(),
                    expected: reading.version,
                    sketch: choice.sketch,
                    edits: ferritecad_document::SketchConstraintEdits {
                        remove: Vec::new(),
                        add: vec![ferritecad_document::AddSketchConstraint::Line(
                            ferritecad_document::AddLineConstraint::Line {
                                curve,
                                kind: ferritecad_document::LineConstraintKind::Horizontal,
                            },
                        )],
                    },
                    destination: PathBuf::new(),
                },
            );
            let current = s.export_path().expect("current");
            let reading = read_extrude_source(&current).expect("reading");
            let target = reading.fillet_bodies[0].target.clone().expect("target");
            let edited = super::add_fillet::add(
                &mut s,
                &ferritecad_jobs::EdgeFilletRequest {
                    source: current,
                    expected: reading.version,
                    body: target.body,
                    fillet: ferritecad_document::EdgeFillet {
                        edge: ferritecad_document::FilletEdge {
                            feature: target.base_feature,
                            joint: target.corners[0].joint,
                        },
                        radius_mm: 2.5,
                    },
                    destination: PathBuf::new(),
                },
            );
            assert!(matches!(edited, Edited::Show(_)), "{}", s.status);
            move_native(&mut s, true);
            move_native(&mut s, false);
        }
        "drawn" => {
            create_native(
                &mut s,
                &private,
                NewDocument::SketchExtrude(
                    ferritecad_jobs::PolygonExtrusion::new(
                        vec![
                            [0., 0.],
                            [60., 0.],
                            [60., 20.],
                            [20., 20.],
                            [20., 40.],
                            [0., 40.],
                        ],
                        9.0,
                    )
                    .expect("polygon"),
                ),
            );
            let current = s.export_path().expect("current");
            let reading = read_extrude_source(&current).expect("reading");
            let sketch = &reading.sketches[0];
            let mut vertices = sketch.vertices.clone().expect("an editable polygon");
            vertices[2].start_mm = [62.0, 21.0];
            apply_sketch_native(
                &mut s,
                ferritecad_jobs::EditSketchRequest {
                    source: current,
                    expected: reading.version,
                    sketch: sketch.sketch,
                    vertices,
                    destination: PathBuf::new(),
                },
            );
        }
        "empty" => create_native(&mut s, &private, NewDocument::Empty),
        other => panic!("unknown child mode {other}"),
    }
    written(&s);
    let current = s.export_path().expect("current");
    let source = Document::open_read_only(&current).expect("current");
    source
        .snapshot_to(&work.join("witness.fcad"))
        .expect("witness");
    source.close().expect("close");
    println!("{SAY}READY");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    // Never decided, never finished: the process is killed with all of it held.
    std::mem::forget(s);
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn crash(mode: &str, root: &Path, work: &Path) {
    let mut child = std::process::Command::new(std::env::current_exe().expect("this binary"))
        .args([
            CHILD,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(MODE, mode)
        .env(ROOT, root)
        .env(WORK, work)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("a child process");
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if line.contains(SAY) {
                let _ = tx.send(());
                break;
            }
        }
    });
    if rx.recv_timeout(Duration::from_secs(600)).is_err() {
        let _ = child.kill();
        panic!("the {mode} child never confirmed its copy");
    }
    // Only our own child, by its own PID.
    child.kill().expect("killed");
    assert!(!child.wait().expect("reaped").success());
}

fn recovery_artifacts(name: &str, stl: &[u8], fbx: &[u8]) {
    if let Some(dir) = std::env::var_os("FCAD_RECOVERY_SESSION_ARTIFACTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join(format!("{name}.stl")), stl).expect("stl");
        std::fs::write(dir.join(format!("{name}.fbx")), fbx).expect("fbx");
    }
}

#[test]
fn native_a_killed_window_recovers_named_untitled_and_edited_models_as_accepted() {
    if !native() {
        return;
    }
    for (mode, name, exported) in [
        ("plate", "plate.fcad", true),
        ("drawn", ferritecad_jobs::UNTITLED, true),
        ("empty", ferritecad_jobs::UNTITLED, false),
    ] {
        let root = tempfile::tempdir().expect("recovery");
        let work = tempfile::tempdir().expect("work");
        let source = work.path().join("plate.fcad");
        plate_file(&source);
        let original = std::fs::read(&source).expect("source");
        crash(mode, root.path(), work.path());
        let witness = work.path().join("witness.fcad");

        // Another process finds it.
        let store = RecoveryStore::open(root.path()).expect("store");
        let found: Vec<_> = store.list().expect("list").recoverable().cloned().collect();
        assert_eq!(found.len(), 1, "{mode}");
        assert_eq!(found[0].name, name, "{mode}");
        assert_eq!(found[0].content, content(&witness), "{mode}");

        // And recovers it the window's way: claim, restore, picture, bind.
        let private = tempfile::tempdir().expect("private");
        let (scene, session) = recover_for_view(
            private.path(),
            &store,
            found[0].record,
            &OperationContext::default(),
        )
        .expect("recovered");
        if exported {
            assert!(!scene.catalogue.is_empty(), "{mode}: a picture");
        }
        let mut s = recording(&store);
        s.bind(Bind::Open(Box::new(session))).expect("bound");
        written(&s);
        assert_eq!(
            s.title(),
            format!("*{name} (recovered) — FerriteCAD"),
            "{mode}"
        );
        assert_eq!(records(root.path()), 1, "{mode}: adopted, not duplicated");
        let current = s.export_path().expect("current");
        assert_eq!(
            sql(&current),
            sql(&witness),
            "{mode}: every SQL cell and ref"
        );

        // The same exports as the model accepted before the crash.
        let alias = s.suggestion().expect("suggestion");
        if exported {
            let (stl, fbx) = export_bytes(&current, &alias, work.path(), &format!("{mode}-rec"));
            let (wstl, wfbx) = export_bytes(&witness, &alias, work.path(), &format!("{mode}-acc"));
            assert_eq!(stl, wstl, "{mode}: STL");
            assert_eq!(fbx, wfbx, "{mode}: FBX");
            recovery_artifacts(&format!("recovered-{mode}"), &stl, &fbx);
        }

        // Saved as a new file, reopened and rebuilt cold like the accepted model.
        let out = work.path().join(format!("{mode}-recovered.fcad"));
        saved(&mut s, SaveTarget::As(out.clone()));
        assert_eq!(sql(&out), sql(&witness), "{mode}: the saved file");
        let words = |path: &Path| {
            let report = cli(&["rebuild".as_ref(), path.as_os_str(), "--cold".as_ref()]);
            String::from_utf8_lossy(&report.stdout)
                .lines()
                .filter(|l| l.contains("resolved") || l.contains("evaluated"))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let rebuilt = words(&out);
        assert!(!exported || !rebuilt.is_empty(), "{mode}: {rebuilt:?}");
        assert_eq!(rebuilt, words(&witness), "{mode}: cold rebuild");
        assert_eq!(std::fs::read(&source).expect("source"), original, "{mode}");
        assert_eq!(settle(&s), Some(RecoveryStatus::Off), "{mode}");
        s.decide_exit();
        s.stop_all();
        assert_eq!(records(root.path()), 0, "{mode}");
    }
    println!("FCAD_30M_NATIVE_CRASH_MATRIX_EXECUTED variants=3");
}

// ---- the real window: comparator, controls and its self-check -----------------

/// What the macOS recipe leaves in its folder, made by the window and nothing else.
const GUI_OUTPUTS: [&str; 6] = [
    "accepted.stl",
    "accepted.fbx",
    "recovered.stl",
    "recovered.fbx",
    "recovered.fcad",
    "empty-recovered.fcad",
];

fn document_id(path: &Path) -> ferritecad_types::DocumentId {
    let document = Document::open_read_only(path).expect("document");
    let id = document.meta().document_id;
    document.close().expect("close");
    id
}

/// Every cell, with the two cells a new document necessarily has of its own.
fn sql_of_a_new_document(
    path: &Path,
) -> std::collections::BTreeMap<String, (Vec<String>, Vec<Vec<rusqlite::types::Value>>)> {
    let mut all = sql(path);
    if let Some((columns, rows)) = all.get_mut("meta") {
        for name in ["document_id", "created_at"] {
            let at = columns.iter().position(|c| c == name).expect(name);
            for row in rows.iter_mut() {
                row[at] = rusqlite::types::Value::Null;
            }
        }
    }
    all
}

/// The window's files against the command line run on the untouched inputs. A
/// missing output is refused before any peer job.
fn compare_gui(root: &Path) {
    let missing: Vec<&str> = GUI_OUTPUTS
        .iter()
        .copied()
        .filter(|name| !root.join(name).is_file())
        .collect();
    assert!(
        missing.is_empty(),
        "missing real GUI output: {}",
        missing.join(", ")
    );
    let read = |name: &str| std::fs::read(root.join(name)).expect(name);
    assert!(
        read("plate.fcad") == read("inputs/plate.fcad"),
        "the source was written"
    );
    assert!(
        read("occupied.fcad") == read("inputs/occupied.fcad"),
        "the occupied file was written"
    );
    let listing = RecoveryStore::at(&root.join("recovery"))
        .expect("store")
        .list()
        .expect("list");
    assert!(
        listing.entries.is_empty() && listing.active == 0,
        "a recovery record was left behind: {} listed, {} active",
        listing.entries.len(),
        listing.active
    );

    let work = tempfile::tempdir().expect("work");
    let source = root.join("inputs/plate.fcad");
    let opened = read_extrude_source(&source).expect("reading");
    let peer = work.path().join("peer.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        source.as_os_str(),
        "--feature".as_ref(),
        opened.features[0].feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        opened.version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        "21.5".as_ref(),
        "-o".as_ref(),
        peer.as_os_str(),
    ]);
    assert!(
        sql(&root.join("recovered.fcad")) == sql(&peer),
        "the recovered file is the edited plate"
    );
    let (peer_stl, peer_fbx) = peer_bytes(&peer, work.path(), "peer");
    assert!(
        read("accepted.stl") == peer_stl,
        "accepted STL against the CLI"
    );
    assert!(
        read("accepted.fbx") == peer_fbx,
        "accepted FBX against the CLI"
    );
    assert!(
        read("recovered.stl") == read("accepted.stl"),
        "recovered STL is the accepted one"
    );
    assert!(
        read("recovered.fbx") == read("accepted.fbx"),
        "recovered FBX is the accepted one"
    );

    let empty = root.join("empty-recovered.fcad");
    let id = document_id(&empty);
    assert!(
        id != document_id(&source) && id != document_id(&root.join("inputs/occupied.fcad")),
        "the recovered empty document is an input"
    );
    let peer_empty = work.path().join("peer-empty.fcad");
    cli(&["create".as_ref(), peer_empty.as_os_str()]);
    assert!(
        sql_of_a_new_document(&empty) == sql_of_a_new_document(&peer_empty),
        "the recovered empty document is an empty document"
    );
}

fn gui_control(root: &Path, name: &str, why: &str, breaks: &dyn Fn(&Path)) {
    let copy = tempfile::tempdir().expect("control");
    for entry in super::add_fillet::walk(root) {
        let to = copy.path().join(entry.strip_prefix(root).expect("inside"));
        std::fs::create_dir_all(to.parent().expect("parent")).expect("dir");
        std::fs::copy(&entry, &to).expect("copy");
    }
    breaks(copy.path());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compare_gui(copy.path())));
    std::panic::set_hook(previous);
    let payload = outcome.expect_err(&format!("negative control {name} accepted"));
    let refused = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_default();
    assert!(
        refused.contains(why),
        "control {name} refused for another reason: {refused}"
    );
}

/// The comparator's positive run and its seven negative controls.
fn compare_with_controls(root: &Path) {
    compare_gui(root);
    let before = cli_runs();
    gui_control(
        root,
        "missing",
        "missing real GUI output: recovered.stl",
        &|r| std::fs::remove_file(r.join("recovered.stl")).expect("remove"),
    );
    assert_eq!(
        cli_runs(),
        before,
        "a peer job ran before the missing output was refused"
    );
    let file = |r: &Path, name: &str| std::fs::read(r.join(name)).expect(name);
    gui_control(root, "source written", "the source was written", &|r| {
        std::fs::write(r.join("plate.fcad"), file(r, "recovered.fcad")).expect("w")
    });
    gui_control(
        root,
        "occupied written",
        "the occupied file was written",
        &|r| std::fs::write(r.join("occupied.fcad"), b"replaced").expect("w"),
    );
    gui_control(
        root,
        "the old document as the recovered one",
        "the recovered file is the edited plate",
        &|r| std::fs::write(r.join("recovered.fcad"), file(r, "inputs/plate.fcad")).expect("w"),
    );
    let work = tempfile::tempdir().expect("work");
    let (old_stl, _) = peer_bytes(&root.join("inputs/plate.fcad"), work.path(), "old");
    gui_control(
        root,
        "an export of the old version",
        "accepted STL against the CLI",
        &|r| {
            for name in ["accepted.stl", "recovered.stl"] {
                std::fs::write(r.join(name), &old_stl).expect("w");
            }
        },
    );
    gui_control(
        root,
        "a record left behind",
        "a recovery record was left behind",
        &|r| {
            let store = RecoveryStore::open(&r.join("recovery")).expect("store");
            let private = tempfile::tempdir().expect("private");
            let session = DocumentSession::open_in(
                private.path(),
                &r.join("inputs/plate.fcad"),
                HistoryLimits::default(),
            )
            .expect("session");
            let mut record = store.create_record().expect("record");
            record
                .publish(1, &session.current(), "plate.fcad")
                .expect("copy");
        },
    );
    gui_control(
        root,
        "an input as the recovered empty document",
        "the recovered empty document is an input",
        &|r| {
            std::fs::write(
                r.join("empty-recovered.fcad"),
                file(r, "inputs/occupied.fcad"),
            )
            .expect("w")
        },
    );
    println!("FCAD_30M_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true");
}

/// The macOS recipe run through the window's own session owner, recorder, claim,
/// restore, picture and save workers, with "the window ended" played by an exit
/// nobody decided. A self-check of the comparator, not window evidence.
#[test]
fn native_recovery_window_scenario_on_session_files_passes_the_comparator_and_its_controls() {
    if !native() {
        return;
    }
    let folder = tempfile::tempdir().expect("root");
    let r = folder.path();
    std::fs::create_dir(r.join("inputs")).expect("inputs");
    let plate = r.join("plate.fcad");
    cli(&[
        "create".as_ref(),
        plate.as_os_str(),
        "--sample".as_ref(),
        "--size".as_ref(),
        "80".as_ref(),
        "40".as_ref(),
        "12".as_ref(),
    ]);
    std::fs::copy(&plate, r.join("inputs/plate.fcad")).expect("pristine");
    let occupied = r.join("occupied.fcad");
    cli(&["create".as_ref(), occupied.as_os_str()]);
    std::fs::copy(&occupied, r.join("inputs/occupied.fcad")).expect("pristine");
    let store = RecoveryStore::open(&r.join("recovery")).expect("store");
    let private = tempfile::tempdir().expect("private");
    let work = tempfile::tempdir().expect("work");

    // 1–3: open, Apply 21.5, export, and the window ends without a decision.
    let mut a = recording(&store);
    a.adopt(
        DocumentSession::open_in(private.path(), &plate, HistoryLimits::default()).expect("open"),
    );
    let feature = read_extrude_source(&plate).expect("reading").features[0].feature;
    apply_native(&mut a, feature, 21.5);
    written(&a);
    let alias = a.suggestion().expect("alias");
    let (stl, fbx) = export_bytes(&a.export_path().expect("current"), &alias, work.path(), "a");
    std::fs::write(r.join("accepted.stl"), stl).expect("stl");
    std::fs::write(r.join("accepted.fbx"), fbx).expect("fbx");
    a.stop_all();

    // 4–6: the next start lists it; Recover; export; Save refused on the occupied
    // name, then Save As; Quit.
    let mut b = recording(&store);
    let found: Vec<_> = store.list().expect("list").recoverable().cloned().collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "plate.fcad");
    let (_, session) = recover_for_view(
        private.path(),
        &store,
        found[0].record,
        &OperationContext::default(),
    )
    .expect("recovered");
    b.bind(Bind::Open(Box::new(session))).expect("bound");
    written(&b);
    let alias = b.suggestion().expect("alias");
    let (stl, fbx) = export_bytes(&b.export_path().expect("current"), &alias, work.path(), "b");
    std::fs::write(r.join("recovered.stl"), stl).expect("stl");
    std::fs::write(r.join("recovered.fbx"), fbx).expect("fbx");
    let (tx, rx) = mpsc::channel();
    let g = b
        .begin_save(SaveTarget::As(occupied.clone()), None, |p, _, c| {
            spawn_save(p, c.clone(), move |v| tx.send(v).expect("save"))
        })
        .expect("start");
    assert!(
        !b.finish_save(g, rx.recv().expect("save"))
            .expect("report")
            .published
    );
    saved(&mut b, SaveTarget::As(r.join("recovered.fcad")));
    b.decide_exit();
    b.stop_all();

    // 7–8: New Empty, and the window ends without a decision.
    let mut c = recording(&store);
    create_native(&mut c, private.path(), NewDocument::Empty);
    written(&c);
    c.stop_all();

    // 9–11: a dirty new document; Recover asks, Cancel stays, Discard recovers.
    let mut d = recording(&store);
    create_native(&mut d, private.path(), NewDocument::Empty);
    written(&d);
    assert_eq!(d.replacing(Some(UnsavedChoice::Cancel)), Replace::Stay);
    assert_eq!(
        d.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    let found: Vec<_> = store.list().expect("list").recoverable().cloned().collect();
    assert_eq!(found.len(), 1, "its own copy is held, not offered");
    let (_, session) = recover_for_view(
        private.path(),
        &store,
        found[0].record,
        &OperationContext::default(),
    )
    .expect("recovered");
    d.bind(Bind::Open(Box::new(session))).expect("bound");
    written(&d);
    assert_eq!(d.title(), "*Untitled (recovered) — FerriteCAD");
    saved(&mut d, SaveTarget::As(r.join("empty-recovered.fcad")));
    d.decide_exit();
    d.stop_all();

    compare_with_controls(r);
}

/// The comparator on the real window's files (`FCAD_30M_GUI_DIR`).
#[test]
fn native_compare_real_recovery_gui_artifacts_with_negative_controls() {
    let Some(root) = std::env::var_os("FCAD_30M_GUI_DIR") else {
        return;
    };
    assert!(native(), "the comparison needs Open CASCADE");
    compare_with_controls(Path::new(&root));
}
