// SPDX-License-Identifier: MIT
//! §30N: named checkpoints through the window's own owners — `Sessions`, the
//! checkpoint worker, the picture worker and the one availability predicate —
//! and, natively, compared with the shipped command line.

use super::fillet::saved;
use super::*;
use crate::checkpoints::availability;
use ferritecad_document::{CheckpointEntry, CircularCut, CutExtent};
use ferritecad_jobs::{RecoveryRecorder, RecoveryStatus, RecoveryStore, SaveTarget, Snapshot};
use ferritecad_types::CheckpointId;
use std::collections::BTreeMap;

fn run_checkpoint(s: &mut Sessions, action: CheckpointAction) -> (u64, Edited) {
    let (tx, rx) = mpsc::channel();
    let generation = s
        .begin_checkpoint(action, |ticket, action, _, cancel| {
            spawn_checkpoint(ticket, action, cancel.clone(), move |result| {
                tx.send(result).expect("deliver")
            })
        })
        .expect("started");
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(120))
        .expect("answer");
    (generation, s.finish_checkpoint(generation, result))
}

/// Create or Delete as the window accepts it: the shown picture is kept and only
/// the version it names moves (`App::keep_picture` without a window).
fn keep(s: &mut Sessions, action: CheckpointAction) {
    let (generation, edited) = run_checkpoint(s, action);
    let Edited::Keep(path) = edited else {
        panic!("not a catalog-only step: {edited:?} {}", s.status);
    };
    let facts = s.commit_kept().expect("accepted with the shown picture");
    assert_eq!(s.export_path().as_deref(), Some(path.as_path()));
    assert_eq!(
        facts.version,
        s.export_source().expect("current").version(),
        "the forms' facts name the new version"
    );
    assert!(s.finish_scene(generation, Ok(())));
}

fn create(s: &mut Sessions, name: &str) -> CheckpointEntry {
    keep(s, CheckpointAction::Create(name.to_owned()));
    listed(s).last().expect("created").clone()
}

fn listed(s: &Sessions) -> Vec<CheckpointEntry> {
    s.export_source()
        .expect("a document")
        .checkpoints()
        .expect("readable")
        .to_vec()
}

fn move_kept(s: &mut Sessions, undo: bool) {
    let (generation, path) = s.begin_move(undo).expect("a step");
    assert!(s.staged_keeps_picture(generation));
    let (tx, rx) = mpsc::channel();
    let worker = spawn_kept_facts(
        path,
        s.scene_token(generation).expect("token"),
        move |result| {
            tx.send(result).expect("answer");
        },
    );
    assert!(s.attach_scene(generation, worker));
    assert!(matches!(
        s.finish_kept_facts(generation, rx.recv().expect("facts")),
        Edited::Keep(_)
    ));
    s.commit_kept().expect("keep the picture without a kernel");
    assert!(s.finish_scene(generation, Ok(())));
}

#[test]
fn checkpoint_steps_keep_the_picture_are_unsaved_and_undo_like_any_step() {
    let f = fixture();
    let original = std::fs::read(&f.file).expect("bytes");
    let mut s = open(&f);
    let a = create(&mut s, "  Twelve high ");
    assert_eq!(a.name, "Twelve high");
    assert!(s.dirty(), "an added checkpoint is not lost to a clean mark");
    assert_eq!(s.title(), "*plate.fcad — FerriteCAD");
    assert!(s.status.starts_with("Checkpoint created"), "{}", s.status);
    assert_eq!(
        a.model,
        s.export_source().expect("current").drawn_model(),
        "it holds the model on screen"
    );

    move_kept(&mut s, true);
    assert!(listed(&s).is_empty() && !s.dirty());
    move_kept(&mut s, false);
    assert_eq!(listed(&s), vec![a.clone()], "Redo: the same UUID");
    assert_eq!(std::fs::read(&f.file).expect("bytes"), original);

    saved(&mut s, SaveTarget::InPlace);
    assert_eq!(
        ferritecad_jobs::list_checkpoints(&f.file)
            .expect("lists")
            .entries,
        vec![a.clone()]
    );

    keep(&mut s, CheckpointAction::Delete(a.id));
    assert!(listed(&s).is_empty() && s.dirty());
    assert!(s.status.starts_with("Checkpoint deleted"), "{}", s.status);
    move_kept(&mut s, true);
    assert_eq!(listed(&s), vec![a], "Undo brings it back");
    assert!(!s.dirty(), "back to what is saved");
}

#[test]
fn a_checkpoint_shares_the_operation_slot_and_a_cancelled_or_late_answer_changes_nothing() {
    let f = fixture();
    let mut s = open(&f);
    let state = |s: &Sessions| {
        (
            s.export_path(),
            s.dirty(),
            s.can_undo(),
            s.session_saved_version(),
            private_files(s),
        )
    };
    let before = state(&s);

    // In flight: nothing else may start, and it may not start beside them.
    let (tx, rx) = mpsc::channel();
    let generation = s
        .begin_checkpoint(
            CheckpointAction::Create("A".into()),
            |ticket, action, _, cancel| {
                spawn_checkpoint(ticket, action, cancel.clone(), move |r| {
                    tx.send(r).expect("deliver")
                })
            },
        )
        .expect("started");
    assert!(s.busy() && !s.can_undo() && !s.can_save_as());
    assert!(
        s.begin_apply(|_, _, _| panic!("no competing edit"))
            .is_none()
    );
    assert!(
        s.begin_checkpoint(CheckpointAction::Create("B".into()), |_, _, _, _| panic!(
            "no second checkpoint"
        ))
        .is_none()
    );
    // Cancel before the answer: the answer is refused and its file goes.
    assert!(s.cancel());
    let result = rx.recv().expect("answer");
    assert_eq!(s.finish_checkpoint(generation, result), Edited::Failed);
    assert_eq!(s.status, "Cancelled; nothing was changed.");
    assert_eq!(state(&s), before);

    // Cancel after the answer, before acceptance: not accepted either.
    let (generation, edited) = run_checkpoint(&mut s, CheckpointAction::Create("A".into()));
    assert!(matches!(edited, Edited::Keep(_)));
    assert!(s.cancel());
    let refused = s.commit_kept().expect_err("cancelled");
    assert!(s.finish_scene(generation, Err(refused)));
    assert_eq!(state(&s), before);

    // An answer about a document that has since been replaced is ignored.
    let (tx, rx) = mpsc::channel();
    let generation = s
        .begin_checkpoint(
            CheckpointAction::Create("late".into()),
            |ticket, action, _, cancel| {
                spawn_checkpoint(ticket, action, cancel.clone(), move |r| {
                    tx.send(r).expect("deliver")
                })
            },
        )
        .expect("started");
    let other = fixture();
    s.adopt(
        DocumentSession::open_in(other.private.path(), &other.file, HistoryLimits::default())
            .expect("another document"),
    );
    let replaced = state(&s);
    let result = rx.recv().expect("answer");
    assert_eq!(s.finish_checkpoint(generation, result), Edited::Ignore);
    assert_eq!(state(&s), replaced);
    assert!(listed(&s).is_empty());

    // A refusal from the library is said in words and changes nothing.
    let (_, edited) = run_checkpoint(&mut s, CheckpointAction::Delete(CheckpointId::new()));
    assert_eq!(edited, Edited::Failed);
    assert!(
        s.status.starts_with("The checkpoint was not deleted"),
        "{}",
        s.status
    );
    assert_eq!(state(&s), replaced);
}

#[test]
fn the_checkpoint_predicate_says_why_and_waits_for_forms_operations_names_and_the_limit() {
    let f = fixture();
    let s = open(&f);
    let current = s.export_source().expect("current");
    let ok = availability(true, false, Some(&current), "A");
    assert_eq!((ok.create.clone(), ok.act.clone()), (Ok(()), Ok(())));
    let reasons = |a: crate::checkpoints::Availability| (a.create.err(), a.act.err());
    let (create, act) = reasons(availability(true, false, None, "A"));
    assert_eq!(create.as_deref(), Some("Open or create a document first."));
    assert_eq!(act, create);
    let (create, act) = reasons(availability(false, false, Some(&current), "A"));
    assert!(
        create
            .expect("busy")
            .starts_with("Wait for the current operation")
    );
    assert!(act.is_some());
    // Restore and Delete wait for an open form, as document Undo does.
    let (create, act) = reasons(availability(true, true, Some(&current), "A"));
    assert!(act.expect("form").starts_with("Close the open form first"));
    assert!(create.is_some());
    let (create, act) = reasons(availability(true, false, Some(&current), "  "));
    assert_eq!(
        create.as_deref(),
        Some("Type a name of 1 to 80 characters, without tabs or line breaks.")
    );
    assert!(act.is_none(), "Restore/Delete do not need a typed name");

    // At the limit Create says so; Restore and Delete still work.
    let dir = tempfile::tempdir().expect("dir");
    let mut source = f.file.clone();
    for index in 0..ferritecad_document::MAX_CHECKPOINTS {
        let next = dir.path().join(format!("{index}.fcad"));
        ferritecad_jobs::create_checkpoint_copy(
            &ferritecad_jobs::CreateCheckpointRequest {
                expected: ferritecad_jobs::read_extrude_source(&source)
                    .expect("reading")
                    .version,
                source,
                name: format!("n{index}"),
                destination: next.clone(),
            },
            &OperationContext::default(),
        )
        .expect("within the limit");
        source = next;
    }
    let full = DocumentSession::open_in(f.private.path(), &source, HistoryLimits::default())
        .expect("opens");
    let (create, act) = reasons(availability(true, false, Some(&full.current()), "one more"));
    assert!(create.expect("full").contains("at most 32 checkpoints"));
    assert!(act.is_none());

    // A list that cannot be read: the model opens, the buttons say why.
    let damaged = dir.path().join("damaged.fcad");
    std::fs::copy(&source, &damaged).expect("copy");
    rusqlite::Connection::open(&damaged)
        .expect("opens")
        .execute(
            "UPDATE checkpoints SET name = 'tab\there' WHERE rowid = 1",
            [],
        )
        .expect("hand edit");
    let opened = DocumentSession::open_in(f.private.path(), &damaged, HistoryLimits::default())
        .expect("the model still opens");
    let snapshot: Arc<Snapshot> = opened.current();
    assert!(snapshot.checkpoints().is_err());
    let (create, act) = reasons(availability(true, false, Some(&snapshot), "A"));
    assert!(
        act.expect("damaged")
            .starts_with("The checkpoint list cannot be read")
    );
    assert!(create.is_some());
}

#[test]
fn a_crash_copy_of_the_window_carries_its_checkpoints() {
    let f = fixture();
    let root = tempfile::tempdir().expect("recovery");
    let store = RecoveryStore::open(root.path()).expect("store");
    let mut s = Sessions::default();
    s.keep_recovery(RecoveryRecorder::start(store.clone(), || {}));
    s.adopt(
        DocumentSession::open_in(f.private.path(), &f.file, HistoryLimits::default())
            .expect("session"),
    );
    let a = create(&mut s, "Before the crash");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while !s.recovery_settled() {
        assert!(std::time::Instant::now() < deadline, "never settled");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(matches!(
        s.recovery_status(),
        Some(RecoveryStatus::Written { .. })
    ));
    let accepted = s.export_source().expect("current").version();
    // The window ends without a decision: the record stays for the next start.
    s.stop_all();
    let record = store
        .list()
        .expect("list")
        .recoverable()
        .next()
        .expect("a record")
        .record;
    let recovered = DocumentSession::recover_in(
        f.private.path(),
        HistoryLimits::default(),
        store.claim(record).expect("claim"),
    )
    .expect("recovered");
    assert_eq!(recovered.current().version(), accepted);
    assert_eq!(
        recovered.current().checkpoints().expect("readable"),
        [a].as_slice()
    );
}

/// Restore without a kernel: listing, Create, Delete and extraction work; the
/// restored model is produced and refused at its picture, and nothing changes.
#[test]
fn stub_checkpoints_work_and_restore_is_refused_at_the_picture() {
    if ferritecad_occt::is_available() {
        return;
    }
    let f = fixture();
    let mut s = open(&f);
    let a = create(&mut s, "A");
    move_kept(&mut s, true);
    assert!(listed(&s).is_empty());
    move_kept(&mut s, false);
    assert_eq!(listed(&s), vec![a.clone()]);
    let (generation, edited) = apply(&mut s, f.feature, 30.0);
    assert!(matches!(edited, Edited::Show(_)));
    s.bind(Bind::Staged).expect("the mock edit is accepted");
    assert!(s.finish_scene(generation, Ok(())));
    let state = (s.export_path(), s.dirty(), s.can_undo(), s.can_redo());
    let (generation, edited) = run_checkpoint(&mut s, CheckpointAction::Restore(a.id));
    let Edited::Show(path) = edited else {
        panic!("a restored model needs its picture: {edited:?}");
    };
    let (tx, rx) = mpsc::channel();
    let worker = spawn_scene(path, s.scene_token(generation).expect("token"), move |v| {
        tx.send(v).expect("scene")
    });
    assert!(s.attach_scene(generation, worker));
    let refused = rx
        .recv()
        .expect("answer")
        .map(|_| ())
        .expect_err("no kernel");
    assert!(s.finish_scene(generation, Err(refused)));
    assert!(
        s.status.starts_with("Could not show the change"),
        "{}",
        s.status
    );
    assert_eq!(
        (s.export_path(), s.dirty(), s.can_undo(), s.can_redo()),
        state
    );
    assert_eq!(height_of(&s.export_path().expect("current")), 30.0);
    let out = f.root.path().join("a.fcad");
    ferritecad_jobs::extract_checkpoint(
        &ferritecad_jobs::ExtractCheckpointRequest {
            source: s.export_path().expect("current"),
            expected: None,
            checkpoint: a.id,
            destination: out.clone(),
        },
        &OperationContext::default(),
    )
    .expect("extraction needs no kernel");
    assert_eq!(height_of(&out), 12.0);
    println!("\nFCAD_30N_STUB_CHECKPOINTS_EXECUTED");
}

/// Open CASCADE without the solver: an unconstrained plate is restored and drawn.
#[test]
fn mixed_checkpoint_restore_draws_with_occt_and_no_solver() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        return;
    }
    let f = fixture();
    let mut s = open(&f);
    let a = create(&mut s, "A");
    apply_native(&mut s, f.feature, 25.0);
    restore_native(&mut s, a.id);
    assert_eq!(height_of(&s.export_path().expect("current")), 12.0);
    assert_eq!(s.export_source().expect("current").drawn_model(), a.model);
    println!("\nFCAD_30N_MIXED_CHECKPOINTS_EXECUTED");
}

/// The window's Restore: the worker, then the picture of the restored model,
/// then the version becoming current with it.
fn restore_native(s: &mut Sessions, id: CheckpointId) {
    let (generation, edited) = run_checkpoint(s, CheckpointAction::Restore(id));
    let Edited::Show(path) = edited else {
        panic!("a restored model is drawn: {edited:?} {}", s.status);
    };
    let (tx, rx) = mpsc::channel();
    let worker = spawn_scene(path, s.scene_token(generation).expect("token"), move |v| {
        tx.send(v).expect("scene")
    });
    assert!(s.attach_scene(generation, worker));
    let scene = rx
        .recv_timeout(std::time::Duration::from_secs(120))
        .expect("scene")
        .expect("the picture of the restored model");
    assert!(!scene.catalogue.is_empty());
    s.bind(Bind::Staged).expect("bind");
    assert!(s.finish_scene(generation, Ok(())));
    assert!(s.status.starts_with("Checkpoint restored"), "{}", s.status);
}

fn add_cut_native(s: &mut Sessions) {
    let reading = read_extrude_source(&s.export_path().expect("current")).expect("reading");
    let body = reading.cut_bodies[0].body;
    let (tx, rx) = mpsc::channel();
    let generation = s
        .begin_apply(|ticket, _, cancel| {
            spawn_add_cut(
                ticket,
                body,
                CircularCut {
                    center_mm: [40.0, 20.0],
                    radius_mm: 5.0,
                    extent: CutExtent::ThroughAll,
                },
                reading.version,
                cancel.clone(),
                move |v| tx.send(v).expect("deliver"),
            )
        })
        .expect("started");
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(120))
        .expect("edit");
    let Edited::Show(path) = s.finish_apply(generation, result) else {
        panic!("the Cut was not added: {}", s.status);
    };
    let (tx, rx) = mpsc::channel();
    let worker = spawn_scene(path, s.scene_token(generation).expect("token"), move |v| {
        tx.send(v).expect("scene")
    });
    assert!(s.attach_scene(generation, worker));
    rx.recv_timeout(std::time::Duration::from_secs(120))
        .expect("scene")
        .expect("picture");
    s.bind(Bind::Staged).expect("bind");
    assert!(s.finish_scene(generation, Ok(())));
}

/// Every row of every table (row identities included) and the schema, with only
/// the named tables and, when asked, the write stamp set aside.
fn all_sql(path: &Path, skip: &[&str], stamp: bool) -> BTreeMap<String, Vec<String>> {
    let mut all: BTreeMap<String, Vec<String>> = crate::fillets::tests::tables(path)
        .into_iter()
        .filter(|(table, _)| !skip.contains(&table.as_str()))
        .map(|(table, (columns, rows))| {
            let rows = rows
                .into_iter()
                .map(|row| {
                    columns
                        .iter()
                        .zip(row)
                        .filter(|(column, _)| {
                            stamp || !(table == "meta" && *column == "modified_at")
                        })
                        .map(|(column, value)| format!("{column}={value:?}"))
                        .collect::<Vec<_>>()
                        .join(";")
                })
                .collect();
            (table, rows)
        })
        .collect();
    let schema =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("opens")
            .prepare("SELECT type, name, tbl_name, sql FROM sqlite_schema ORDER BY type, name")
            .expect("schema")
            .query_map([], |row| {
                Ok(format!(
                    "{}|{}|{}|{:?}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?
                ))
            })
            .expect("schema")
            .collect::<rusqlite::Result<_>>()
            .expect("schema");
    all.insert("<sqlite_schema>".to_owned(), schema);
    all
}

/// An independent reading of a binary STL: triangle count, the z extent and the
/// enclosed volume (divergence theorem), nothing from the writer.
fn stl_facts(bytes: &[u8]) -> (usize, f64, f64) {
    let count = u32::from_le_bytes(bytes[80..84].try_into().expect("count")) as usize;
    assert_eq!(bytes.len(), 84 + 50 * count, "a binary STL");
    let (mut low, mut high, mut volume) = (f64::MAX, f64::MIN, 0.0);
    for triangle in 0..count {
        let at = 84 + 50 * triangle + 12;
        let vertex = |k: usize| -> [f64; 3] {
            let mut v = [0.0; 3];
            for (axis, value) in v.iter_mut().enumerate() {
                let o = at + 12 * k + 4 * axis;
                *value = f64::from(f32::from_le_bytes(bytes[o..o + 4].try_into().expect("f32")));
            }
            v
        };
        let [a, b, c] = [vertex(0), vertex(1), vertex(2)];
        for v in [a, b, c] {
            low = low.min(v[2]);
            high = high.max(v[2]);
        }
        volume += (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0]))
            / 6.0;
    }
    (count, high - low, volume.abs())
}

fn ids_and_refs(path: &Path) -> (Vec<ObjectId>, Vec<String>) {
    let document = Document::open_read_only(path).expect("document");
    let mut ids: Vec<ObjectId> = document
        .objects()
        .expect("objects")
        .into_iter()
        .map(|o| o.id)
        .collect();
    ids.sort();
    let mut refs: Vec<String> = document
        .topology_refs()
        .expect("refs")
        .iter()
        .map(|r| format!("{r:?}"))
        .collect();
    refs.sort();
    document.close().expect("close");
    (ids, refs)
}

/// The whole §30N route on Open CASCADE and PlaneGCS, through the window's own
/// workers, against the shipped command line.
#[test]
fn native_checkpoints_restore_undo_redo_save_reopen_and_extract_like_the_command_line() {
    if !native() {
        return;
    }
    let f = fixture();
    let original = std::fs::read(&f.file).expect("bytes");
    let mut s = open(&f);
    let before_a = s.export_source().expect("version A is made from");
    let a = create(&mut s, "Plate, 12 high");
    apply_native(&mut s, f.feature, 20.0);
    add_cut_native(&mut s);
    let before_b = s.export_source().expect("version B is made from");
    let b = create(&mut s, "Plate, 20 high, one hole");
    let at_b = s.export_source().expect("with B");
    assert_ne!(a.model, b.model);
    assert_eq!(
        std::fs::read(&f.file).expect("bytes"),
        original,
        "nothing saved yet"
    );

    restore_native(&mut s, a.id);
    let restored = s.export_source().expect("restored");
    assert_eq!(restored.drawn_model(), a.model);
    assert_eq!(restored.drawn_model(), before_a.drawn_model());
    assert_eq!(listed(&s), vec![a.clone(), b.clone()], "the list is kept");
    assert_eq!(
        ids_and_refs(restored.path()),
        ids_and_refs(before_a.path()),
        "UUIDs and refs"
    );
    assert_eq!(height_of(restored.path()), 12.0);
    assert!(s.dirty());
    assert_eq!(
        std::fs::read(&f.file).expect("bytes"),
        original,
        "Restore saves nothing"
    );

    move_native(&mut s, true);
    assert_eq!(s.export_path().as_deref(), Some(at_b.path()));
    assert_eq!(height_of(at_b.path()), 20.0);
    move_native(&mut s, false);
    assert_eq!(
        s.export_path().as_deref(),
        Some(restored.path()),
        "Redo: the same version"
    );

    // Unsaved exports of the restored model, by the window's own export workers.
    let work = tempfile::tempdir().expect("work");
    let (window_stl, window_fbx) = export_bytes(restored.path(), &f.file, work.path(), "restored");

    saved(&mut s, SaveTarget::InPlace);
    drop(s);
    let reopened = DocumentSession::open_in(f.private.path(), &f.file, HistoryLimits::default())
        .expect("reopens");
    assert_eq!(
        reopened.current().checkpoints().expect("readable"),
        [a.clone(), b.clone()].as_slice()
    );
    assert_eq!(reopened.current().drawn_model(), a.model);

    // The command line lists the saved file and extracts both models.
    let listing = cli(&["list-checkpoints".as_ref(), f.file.as_os_str()]);
    let text = String::from_utf8(listing.stdout).expect("text");
    let line = |id: CheckpointId| {
        text.lines()
            .find(|line| line.starts_with(&id.to_string()))
            .expect("listed")
            .to_owned()
    };
    assert!(line(a.id).ends_with("Plate, 12 high  (the model as it is now)"));
    assert!(line(b.id).ends_with("Plate, 20 high, one hole"));
    let extract = |entry: &CheckpointEntry, tag: &str| {
        let out = work.path().join(format!("{tag}.fcad"));
        cli(&[
            "extract-checkpoint".as_ref(),
            f.file.as_os_str(),
            "--checkpoint".as_ref(),
            entry.id.to_string().as_ref(),
            "--output".as_ref(),
            out.as_os_str(),
        ]);
        out
    };
    let cli_a = extract(&a, "cli-a");
    let cli_b = extract(&b, "cli-b");

    // SQL: an extracted checkpoint is the version it was made from, every cell,
    // row identity, stamp and schema entry; the only difference allowed for B is
    // the catalog, because an image never contains checkpoints.
    assert_eq!(
        all_sql(&cli_a, &[], true),
        all_sql(before_a.path(), &[], true)
    );
    assert_eq!(
        all_sql(&cli_b, &["checkpoints"], true),
        all_sql(before_b.path(), &["checkpoints"], true)
    );
    assert!(all_sql(&cli_b, &[], true)["checkpoints"].is_empty());
    // The restored version is A's model with the kept catalog and a new stamp.
    assert_eq!(
        all_sql(restored.path(), &["checkpoints"], false),
        all_sql(&cli_a, &["checkpoints"], false)
    );

    // Geometry, read independently: A is the plain 80 x 40 x 12 plate, B is 20
    // high with a 5 mm hole through it.
    let (cli_a_stl, cli_a_fbx) = peer_bytes(&cli_a, work.path(), "a");
    let (cli_b_stl, cli_b_fbx) = peer_bytes(&cli_b, work.path(), "b");
    assert_eq!(window_stl, cli_a_stl, "the window's unsaved STL is A's");
    assert_eq!(window_fbx, cli_a_fbx, "the window's unsaved FBX is A's");
    let (triangles_a, height_a, volume_a) = stl_facts(&cli_a_stl);
    let (triangles_b, height_b, volume_b) = stl_facts(&cli_b_stl);
    assert!((height_a - 12.0).abs() < 1e-4 && (height_b - 20.0).abs() < 1e-4);
    assert!((volume_a - 80.0 * 40.0 * 12.0).abs() < 1e-2, "{volume_a}");
    let hole = std::f64::consts::PI * 25.0 * 20.0;
    assert!(
        (volume_b - (80.0 * 40.0 * 20.0 - hole)).abs() < 0.05 * hole,
        "{volume_b}"
    );
    assert!(triangles_b > triangles_a, "the hole adds faces");
    if let Ok(dir) = std::env::var("FCAD_CHECKPOINT_SESSION_ARTIFACTS") {
        let dir = PathBuf::from(dir);
        for (name, stl, fbx) in [
            ("checkpoint-a", &cli_a_stl, &cli_a_fbx),
            ("checkpoint-b", &cli_b_stl, &cli_b_fbx),
        ] {
            std::fs::write(dir.join(format!("{name}.stl")), stl).expect("artifact");
            std::fs::write(dir.join(format!("{name}.fbx")), fbx).expect("artifact");
        }
    }
    println!(
        "\nFCAD_30N_NATIVE_CHECKPOINTS_EXECUTED volume_a={volume_a:.3} volume_b={volume_b:.3}"
    );
}

/// A checkpoint of an imported STEP part holds the bytes the document stores;
/// it needs no STEP file anywhere once made.
#[test]
fn native_an_imported_step_checkpoint_needs_no_step_file() {
    if !native() {
        return;
    }
    let work = tempfile::tempdir().expect("work");
    let step = work.path().join("part.step");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/step/canonical/01-single-part.step");
    std::fs::copy(&fixture, &step).expect("a private STEP copy");
    let document = work.path().join("imported.fcad");
    let imported = std::process::Command::new(crate::creates::tests::ferritecad())
        .args([
            "import-step".as_ref(),
            step.as_os_str(),
            "-o".as_ref(),
            document.as_os_str(),
        ])
        .output()
        .expect("import-step");
    assert!(
        matches!(imported.status.code(), Some(0 | 4)),
        "{imported:?}"
    );
    std::fs::remove_file(&step).expect("only the private copy goes");

    let private = tempfile::tempdir().expect("private");
    let mut s = Sessions::default();
    s.adopt(
        DocumentSession::open_in(private.path(), &document, HistoryLimits::default())
            .expect("session"),
    );
    let a = create(&mut s, "As imported");
    saved(&mut s, SaveTarget::InPlace);
    let out = work.path().join("extracted.fcad");
    cli(&[
        "extract-checkpoint".as_ref(),
        document.as_os_str(),
        "--checkpoint".as_ref(),
        a.id.to_string().as_ref(),
        "--output".as_ref(),
        out.as_os_str(),
    ]);
    assert!(!step.exists());
    let tables = crate::fillets::tests::tables(&out);
    let (columns, rows) = &tables["imported_sources"];
    let bytes = columns.iter().position(|c| c == "bytes").expect("bytes");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0][bytes],
        rusqlite::types::Value::Blob(std::fs::read(&fixture).expect("fixture")),
        "the stored source bytes, unchanged"
    );
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let scene = ferritecad_scene::snapshot_of(
        &out,
        &mut kernel,
        |kernel, source| kernel.import_step(source),
        &TessellationParams::default(),
        &OperationContext::default(),
    )
    .expect("drawn from the stored bytes alone");
    assert!(!scene.catalogue.is_empty());
    println!("\nFCAD_30N_NATIVE_STEP_CHECKPOINT_EXECUTED");
}

/// What the macOS recipe leaves behind; made by the real window only.
const GUI_OUTPUTS: [&str; 4] = [
    "plate.fcad",
    "restored.stl",
    "restored.fbx",
    "old-saved.fcad",
];

fn schema_of(path: &Path) -> i64 {
    rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("opens")
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("reads")
}

/// Compares the outputs of the window recipe in `root` (see the verification
/// record) with the shipped command line. Reads only what the window wrote.
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
        read("old.fcad") == read("inputs/old.fcad"),
        "the old file was written"
    );
    assert_eq!(
        schema_of(&root.join("old.fcad")),
        3,
        "the old file was written"
    );

    // The saved plate: the restored model of A, with A and B kept in order.
    let plate = root.join("plate.fcad");
    let listing = ferritecad_jobs::list_checkpoints(&plate).expect("lists");
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["A 12 mm", "B 20 mm hole"], "the saved checkpoints");
    let (a, b) = (&listing.entries[0], &listing.entries[1]);
    assert_eq!(listing.model, a.model, "the saved model is not A's");
    let work = tempfile::tempdir().expect("work");
    let extract = |source: &Path, entry: &CheckpointEntry, tag: &str| {
        let out = work.path().join(format!("{tag}.fcad"));
        cli(&[
            "extract-checkpoint".as_ref(),
            source.as_os_str(),
            "--checkpoint".as_ref(),
            entry.id.to_string().as_ref(),
            "--output".as_ref(),
            out.as_os_str(),
        ]);
        out
    };
    let cli_a = extract(&plate, a, "a");
    let cli_b = extract(&plate, b, "b");
    // A was made from the file as it was opened: every cell, stamp included.
    assert!(
        all_sql(&cli_a, &[], true) == all_sql(&root.join("inputs/plate.fcad"), &[], true),
        "checkpoint A is not the opened plate"
    );
    assert_eq!(height_of(&cli_a), 12.0);
    let b_reading = read_extrude_source(&cli_b).expect("reading");
    assert_eq!(
        b_reading.features[0].distance_mm,
        Some(20.0),
        "checkpoint B height"
    );
    // The catalogue has a row for every feature, including the NewBody base
    // with a local refusal; only a saved circular-cut row names a Cut.
    let cuts: Vec<_> = b_reading
        .cut_features
        .iter()
        .filter_map(|row| row.saved.as_ref())
        .collect();
    assert_eq!(cuts.len(), 1, "checkpoint B has one Cut");
    assert_eq!(cuts[0].center_mm, [40.0, 20.0]);
    assert_eq!(cuts[0].radius_mm, 5.0);
    assert_eq!(cuts[0].extent, CutExtent::ThroughAll);
    assert!(
        ids_and_refs(&cli_b).0.len() > ids_and_refs(&cli_a).0.len(),
        "B adds the Cut's objects"
    );
    // The restored, saved model is A's in every cell but the stamp and the list.
    assert!(
        all_sql(&plate, &["checkpoints"], false) == all_sql(&cli_a, &["checkpoints"], false),
        "the saved plate is not checkpoint A's model"
    );

    // The window's unsaved exports of the restored model are A's.
    let (stl, fbx) = peer_bytes(&cli_a, work.path(), "a");
    assert!(read("restored.stl") == stl, "restored STL against the CLI");
    assert!(read("restored.fbx") == fbx, "restored FBX against the CLI");
    let (b_stl, _) = peer_bytes(&cli_b, work.path(), "b");
    let (a_triangles, a_height, a_volume) = stl_facts(&stl);
    let (b_triangles, b_height, b_volume) = stl_facts(&b_stl);
    assert!((a_height - 12.0).abs() < 1e-4 && (b_height - 20.0).abs() < 1e-4);
    assert!((a_volume - 80.0 * 40.0 * 12.0).abs() < 1e-2);
    let hole = std::f64::consts::PI * 25.0 * 20.0;
    assert!((b_volume - (80.0 * 40.0 * 20.0 - hole)).abs() < 0.05 * hole);
    assert!(b_triangles > a_triangles, "checkpoint B's hole adds faces");

    // The old v3 file: read as it was, saved elsewhere as v4 with its checkpoint.
    let old_saved = root.join("old-saved.fcad");
    assert_eq!(schema_of(&old_saved), 4);
    let old = ferritecad_jobs::list_checkpoints(&old_saved).expect("lists");
    assert_eq!(old.entries.len(), 1, "the old file's checkpoint");
    assert_eq!(old.entries[0].name, "old");
    let v3 = Document::open_read_only(root.join("old.fcad")).expect("v3 reads");
    assert_eq!(
        old.entries[0].model,
        v3.model_without_checkpoints().expect("hash"),
        "the old file's checkpoint holds the old file's model"
    );
    v3.close().expect("close");

    let store = ferritecad_jobs::RecoveryStore::open(&root.join("recovery")).expect("store");
    assert_eq!(
        store.list().expect("list").recoverable().count(),
        0,
        "a recovery record was left behind"
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

/// The real window's outputs in `FCAD_30N_GUI_DIR` against the command line, then
/// four negative controls on copies of those same outputs. Nothing here makes a
/// window output; without the variable the test does nothing.
#[test]
fn native_compare_real_checkpoint_gui_artifacts_with_negative_controls() {
    let Some(root) = std::env::var_os("FCAD_30N_GUI_DIR") else {
        return;
    };
    assert!(native(), "the comparison needs Open CASCADE");
    let root = Path::new(&root);
    compare_gui(root);
    let before = cli_runs();
    gui_control(
        root,
        "missing",
        "missing real GUI output: restored.stl",
        &|r| std::fs::remove_file(r.join("restored.stl")).expect("remove"),
    );
    assert_eq!(
        cli_runs(),
        before,
        "a peer job ran before a missing output was refused"
    );
    let file = |r: &Path, name: &str| std::fs::read(r.join(name)).expect(name);
    gui_control(root, "unsaved plate", "the saved checkpoints", &|r| {
        std::fs::write(r.join("plate.fcad"), file(r, "inputs/plate.fcad")).expect("w")
    });
    gui_control(
        root,
        "old file upgraded",
        "the old file was written",
        &|r| std::fs::write(r.join("old.fcad"), file(r, "old-saved.fcad")).expect("w"),
    );
    gui_control(
        root,
        "an export of B",
        "restored STL against the CLI",
        &|r| {
            let listing = ferritecad_jobs::list_checkpoints(&r.join("plate.fcad")).expect("lists");
            let out = r.join("b-control.fcad");
            ferritecad_jobs::extract_checkpoint(
                &ferritecad_jobs::ExtractCheckpointRequest {
                    source: r.join("plate.fcad"),
                    expected: None,
                    checkpoint: listing.entries[1].id,
                    destination: out.clone(),
                },
                &OperationContext::default(),
            )
            .expect("B");
            let (stl, _) = peer_bytes(&out, r, "b-control");
            std::fs::write(r.join("restored.stl"), stl).expect("w");
        },
    );
    println!("\nFCAD_30N_GUI_COMPARE_OK negative_controls=4 all_SQL_cells=true");
}
