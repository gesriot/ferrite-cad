// SPDX-License-Identifier: MIT
//! §30L: a new document is a session before it is a file. Every `NewDocument`
//! variant goes through the window's own create worker (`create_for_view`), is
//! bound the way Open binds a session, exports before its first Save, is saved
//! for the first time by the existing no-clobber Save As, reopened and rebuilt
//! cold, and compared with the shipped command line's `create*` of the same
//! content. Two independent creations share no identity, so the comparison maps
//! exactly the new identities (all of them, by position) and nothing else; within
//! one session (snapshot → first Save → reopen) nothing is remapped.
use super::add_cut::{Id, sql_mapped};
use super::fillet::{saved, sql, state};
use super::*;
use ferritecad_jobs::{
    AnnularExtrusion, CircleExtrusion, FullTurnRevolution, PolygonExtrusion, RevolveAngle,
    SaveTarget,
};
use std::collections::BTreeMap;
use std::ffi::OsString;

/// One of the seven variants: what the window asks for, and how the command
/// line is asked for the same content (`{out}` is the destination).
struct Variant {
    name: &'static str,
    content: NewDocument,
    /// CLI arguments, and the JSON request when the command takes one.
    cli: Vec<&'static str>,
    request: Option<&'static str>,
    /// Whether the document has an Extrude whose height the session can edit.
    extrude: bool,
}

const L_POLYGON: [[f64; 2]; 6] = [
    [0., 0.],
    [60., 0.],
    [60., 20.],
    [20., 20.],
    [20., 40.],
    [0., 40.],
];
const STEPPED: [[f64; 2]; 6] = [
    [4., 0.],
    [10., 0.],
    [10., 5.],
    [7., 5.],
    [7., 15.],
    [4., 15.],
];

fn variants() -> Vec<Variant> {
    vec![
        Variant {
            name: "empty",
            content: NewDocument::Empty,
            cli: vec!["create"],
            request: None,
            extrude: false,
        },
        Variant {
            name: "plate",
            content: NewDocument::SamplePlate(PlateSize {
                width: 83.,
                depth: 47.,
                height: 13.,
            }),
            cli: vec!["create", "--sample", "--size", "83", "47", "13"],
            request: None,
            extrude: true,
        },
        Variant {
            name: "polygon",
            content: NewDocument::SketchExtrude(
                PolygonExtrusion::new(L_POLYGON.to_vec(), 10.).expect("polygon"),
            ),
            cli: vec!["create-sketch-extrude"],
            request: Some(
                r#"{"request_version":1,"points_mm":[[0,0],[60,0],[60,20],[20,20],[20,40],[0,40]],"height_mm":10}"#,
            ),
            extrude: true,
        },
        Variant {
            name: "circle",
            content: NewDocument::CircleExtrude(
                CircleExtrusion::new([12., -7.], 10., 15.).expect("circle"),
            ),
            cli: vec!["create-circle-extrude"],
            request: Some(
                r#"{"schema_version":1,"center_mm":[12.0,-7.0],"radius_mm":10.0,"height_mm":15.0}"#,
            ),
            extrude: true,
        },
        Variant {
            name: "annulus",
            content: NewDocument::AnnularExtrude(
                AnnularExtrusion::new([12., -7.], 10., 4., 15.).expect("annulus"),
            ),
            cli: vec!["create-annular-extrude"],
            request: Some(concat!(
                r#"{"schema_version":1,"center_mm":[12.0,-7.0],"#,
                r#""outer_radius_mm":10.0,"inner_radius_mm":4.0,"height_mm":15.0}"#
            )),
            extrude: true,
        },
        Variant {
            name: "revolve",
            content: NewDocument::SketchRevolve(
                FullTurnRevolution::new(STEPPED.to_vec()).expect("revolve"),
            ),
            cli: vec!["create-sketch-revolve"],
            request: Some(concat!(
                r#"{"request_version":1,"points_mm":[[4,0],[10,0],[10,5],[7,5],[7,15],[4,15]],"#,
                r#""axis":"sketch_y","angle":"full_turn"}"#
            )),
            extrude: false,
        },
        Variant {
            name: "partial-revolve",
            content: NewDocument::SketchPartialRevolve {
                profile: FullTurnRevolution::new(STEPPED.to_vec()).expect("revolve"),
                angle: RevolveAngle::new(137.5).expect("angle"),
            },
            cli: vec!["create-sketch-revolve"],
            request: Some(concat!(
                r#"{"request_version":2,"points_mm":[[4,0],[10,0],[10,5],[7,5],[7,15],[4,15]],"#,
                r#""axis":"sketch_y","extent":{"kind":"angle","degrees":137.5}}"#
            )),
            extrude: false,
        },
    ]
}

/// The shipped command line's own creation of the same content, with its JSON
/// answer read back: the contract is unchanged (exit 0, the asked path published).
fn cli_create(variant: &Variant, root: &Path) -> PathBuf {
    let out = root.join(format!("{}-cli.fcad", variant.name));
    let mut args: Vec<OsString> = vec![variant.cli[0].into()];
    if let Some(request) = variant.request {
        let file = root.join(format!("{}-request.json", variant.name));
        std::fs::write(&file, request).expect("request");
        args.extend([
            file.into_os_string(),
            "-o".into(),
            out.clone().into_os_string(),
        ]);
        args.push("--json".into());
    } else {
        args.push(out.clone().into_os_string());
        args.extend(variant.cli[1..].iter().map(OsString::from));
        args.push("--json".into());
    }
    let refs: Vec<&std::ffi::OsStr> = args.iter().map(OsString::as_os_str).collect();
    let output = cli(&refs);
    let answer = String::from_utf8_lossy(&output.stdout);
    assert!(answer.contains(r#""ok":true"#), "{answer}");
    assert!(answer.contains(r#""destination":"#), "{answer}");
    assert!(out.is_file());
    out
}

/// The window's create worker, then the binding Open uses: the candidate becomes
/// the session only here, as when its picture is accepted.
fn create_into(s: &mut Sessions, root: &Path, content: NewDocument) {
    let (scene, session) =
        create_for_view(root, content, &OperationContext::default()).expect("the candidate");
    assert!(session.is_untitled());
    let _ = scene;
    s.bind(Bind::Open(Box::new(session))).expect("bound");
}

/// Every 16-byte identity in `path`, in table, rowid and column order: a blob of
/// exactly 16 bytes, or a CBOR byte string of 16 bytes inside a payload.
fn identities(path: &Path) -> Vec<Id> {
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("db");
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
        .expect("tables")
        .query_map([], |r| r.get(0))
        .expect("names")
        .collect::<std::result::Result<_, _>>()
        .expect("names");
    let mut found = Vec::new();
    for table in tables {
        // A table without rowid holds only identities already found in the
        // tables that name them (`deps`); it is still compared cell for cell.
        let Ok(mut q) = db.prepare(&format!("SELECT * FROM \"{table}\" ORDER BY rowid")) else {
            continue;
        };
        let n = q.column_count();
        // Hashes are derived from the payloads (and checked by `sql_mapped`);
        // only the payload and identity cells are scanned.
        let scanned: Vec<bool> = (0..n)
            .map(|i| q.column_name(i).is_ok_and(|c| c == "payload"))
            .collect();
        let rows: Vec<Vec<rusqlite::types::Value>> = q
            .query_map([], |r| (0..n).map(|i| r.get(i)).collect())
            .expect("rows")
            .collect::<std::result::Result<_, _>>()
            .expect("rows");
        for row in rows {
            for (i, value) in row.into_iter().enumerate() {
                let rusqlite::types::Value::Blob(blob) = value else {
                    continue;
                };
                if let Ok(id) = <Id>::try_from(&blob[..]) {
                    found.push(id);
                    continue;
                }
                if !scanned[i] {
                    continue;
                }
                let mut i = 0;
                while i + 17 <= blob.len() {
                    if blob[i] == 0x50 {
                        found.push(<Id>::try_from(&blob[i + 1..i + 17]).expect("16 bytes"));
                        i += 17;
                    } else {
                        i += 1;
                    }
                }
            }
        }
    }
    found
}

/// The proved bijection between two independent creations of one content: the
/// identities in the same places, consistently, one to one, and none shared.
fn pair_independent(ours: &Path, theirs: &Path) -> BTreeMap<Id, Id> {
    let (a, b) = (identities(ours), identities(theirs));
    assert_eq!(a.len(), b.len(), "the same number of identity places");
    let mut map = BTreeMap::new();
    let mut back = BTreeMap::new();
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(
            *map.entry(*x).or_insert(*y),
            *y,
            "an identity would pair with two"
        );
        assert_eq!(
            *back.entry(*y).or_insert(*x),
            *x,
            "two identities would pair with one"
        );
    }
    for x in map.keys() {
        assert!(
            !back.contains_key(x),
            "an identity is shared by two documents"
        );
    }
    map
}

/// Every SQL cell under the map, with the two stamps an independent creation
/// writes for itself (`created_at`, `modified_at`) set aside.
fn independent_cells(path: &Path, map: &BTreeMap<Id, Id>) -> BTreeMap<String, Vec<String>> {
    let mut all = sql_mapped(path, map);
    for row in all.get_mut("meta").expect("meta") {
        let kept: String = row
            .split(';')
            .filter(|cell| !cell.is_empty() && !cell.starts_with("created_at="))
            .map(|cell| format!("{cell};"))
            .collect();
        *row = kept;
    }
    all
}

fn fbx_mapped(fbx: &[u8], map: &BTreeMap<Id, Id>) -> String {
    let mut text = String::from_utf8(fbx.to_vec()).expect("ASCII FBX");
    for (from, to) in map {
        let (from, to) = (
            ObjectId::from_bytes(*from).expect("id").to_string(),
            ObjectId::from_bytes(*to).expect("id").to_string(),
        );
        text = text.replace(&from, &to);
    }
    text
}

fn artifacts(name: &str, stl: &[u8], fbx: &[u8]) {
    if let Some(dir) = std::env::var_os("FCAD_NEW_DOCUMENT_SESSION_ARTIFACTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join(format!("{name}.stl")), stl).expect("stl");
        std::fs::write(dir.join(format!("{name}.fbx")), fbx).expect("fbx");
    }
}

fn first_extrude(path: &Path) -> ObjectId {
    ferritecad_jobs::read_extrude_source(path)
        .expect("reading")
        .features[0]
        .feature
}

/// The matrix: each variant, created, edited where it has a height, exported
/// before Save, saved for the first time, reopened, rebuilt cold and compared
/// with the command line's own creation.
fn matrix() {
    let root = tempfile::tempdir().expect("root");
    let sessions_root = tempfile::tempdir().expect("sessions");
    for variant in variants() {
        let name = variant.name;
        let theirs = cli_create(&variant, root.path());
        let mut s = Sessions::default();
        create_into(&mut s, sessions_root.path(), variant.content.clone());

        // Untitled, unsaved, named without its private folder.
        assert_eq!(s.logical_path(), None, "{name}");
        assert!(s.untitled() && s.dirty() && s.can_save(), "{name}");
        assert_eq!(s.name().as_deref(), Some("Untitled"));
        assert_eq!(s.title(), "*Untitled — FerriteCAD");
        let private = s.private_directory().expect("private").to_path_buf();
        assert_eq!(shown_as(&private.join("v0.fcad")), "Untitled");
        assert!(!s.can_undo() && !s.can_redo());
        let created = s.export_path().expect("current");

        // The same model as the command line's, under the new-identity bijection.
        let map = pair_independent(&created, &theirs);
        assert_eq!(
            independent_cells(&created, &map),
            independent_cells(&theirs, &BTreeMap::new()),
            "{name}: every SQL cell but the stamps and the proved new-identity bijection"
        );

        // A step and its Undo/Redo before any Save: never saved, no remap.
        if variant.extrude {
            apply_native(&mut s, first_extrude(&created), 7.5);
            assert!(s.can_undo() && s.dirty());
            move_native(&mut s, true);
            assert_eq!(sql(&s.export_path().expect("undone")), sql(&created));
            assert!(
                s.dirty(),
                "{name}: Undo to the created version is not a save"
            );
            move_native(&mut s, false);
            move_native(&mut s, true);
        }
        let current = s.export_path().expect("current");

        // Exports before Save read the accepted private version.
        let mut before = None;
        if name != "empty" {
            let alias = s.suggestion().expect("suggestion");
            assert!(
                !alias.starts_with(&private),
                "{name}: suggested the private folder"
            );
            assert_eq!(alias.file_name().and_then(|n| n.to_str()), Some("Untitled"));
            let (stl, fbx) =
                export_bytes(&current, &alias, root.path(), &format!("{name}-unsaved"));
            let (peer_stl, peer_fbx) = peer_bytes(&theirs, root.path(), &format!("{name}-peer"));
            assert_eq!(stl, peer_stl, "{name}: unsaved STL against the CLI");
            assert_eq!(
                fbx_mapped(&fbx, &map),
                String::from_utf8(peer_fbx).expect("ASCII"),
                "{name}: unsaved FBX against the CLI under the bijection"
            );
            if matches!(name, "polygon" | "annulus") {
                artifacts(&format!("new-{name}-before-save"), &stl, &fbx);
            }
            before = Some((stl, fbx));
        }

        // The first Save: the exact accepted version, under its new name.
        let file = root.path().join(format!("{name}.fcad"));
        let identity = Document::open_read_only(&current)
            .expect("current")
            .meta()
            .document_id;
        saved(&mut s, SaveTarget::As(file.clone()));
        assert_eq!(s.logical_path(), Some(file.as_path()));
        assert!(!s.untitled() && !s.dirty());
        assert_eq!(s.title(), format!("{name}.fcad — FerriteCAD"));
        assert_eq!(
            sql(&file),
            sql(&current),
            "{name}: the saved file is the version"
        );
        let reopened = DocumentSession::open(&file).expect("reopen");
        assert!(!reopened.is_dirty());
        assert_eq!(
            Document::open_read_only(&file)
                .expect("saved")
                .meta()
                .document_id,
            identity,
            "{name}: the first Save kept the document's identity"
        );
        drop(reopened);
        let ours_report = cli(&["rebuild".as_ref(), file.as_os_str(), "--cold".as_ref()]);
        let their_report = cli(&["rebuild".as_ref(), theirs.as_os_str(), "--cold".as_ref()]);
        let words = |o: &std::process::Output| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| l.contains("resolved") || l.contains("evaluated"))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            words(&ours_report),
            words(&their_report),
            "{name}: cold rebuild"
        );

        // After Save the same exports, from the saved file; the history is kept.
        if let Some((stl, fbx)) = before {
            let (saved_stl, saved_fbx) =
                export_bytes(&file, &file, root.path(), &format!("{name}-saved"));
            assert_eq!(
                (&saved_stl, &saved_fbx),
                (&stl, &fbx),
                "{name}: saved exports"
            );
            if matches!(name, "polygon" | "annulus") {
                artifacts(&format!("new-{name}-after-save"), &saved_stl, &saved_fbx);
            }
        }
        if variant.extrude {
            assert!(s.can_redo(), "{name}: the history survived the first Save");
            move_native(&mut s, false);
            assert!(
                s.dirty(),
                "{name}: a version other than the saved one is unsaved"
            );
            move_native(&mut s, true);
            assert!(!s.dirty());
        }
        s.stop_all();
        assert!(!private.exists(), "{name}: the session's folder stayed");
    }
}

#[test]
fn native_every_new_document_variant_is_untitled_until_its_first_save_like_the_cli() {
    if !native() {
        return;
    }
    matrix();
}

/// OCCT without the solver: none of the seven needs it (no profile is constrained).
#[test]
fn mixed_new_documents_need_no_solver() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        eprintln!("skipped: requires OCCT/no solver");
        return;
    }
    matrix();
}

/// Without Open CASCADE the kernel-free document is still made, but no picture
/// can be read: nothing is bound, the previous session stays and the candidate's
/// folder is gone.
#[test]
fn stub_new_document_is_refused_at_the_picture_and_keeps_the_open_one() {
    if ferritecad_occt::is_available() {
        eprintln!("skipped: requires stub");
        return;
    }
    let f = fixture();
    let mut s = open(&f);
    let before = state(&s);
    let sessions_root = tempfile::tempdir().expect("sessions");
    for content in [
        NewDocument::Empty,
        NewDocument::SamplePlate(PlateSize::DEFAULT),
    ] {
        let error = create_for_view(sessions_root.path(), content, &OperationContext::default())
            .expect_err("no picture");
        assert_eq!(error.kind(), ErrorKind::Unsupported, "{error}");
    }
    assert!(
        std::fs::read_dir(sessions_root.path())
            .expect("dir")
            .next()
            .is_none()
    );
    assert_eq!(state(&s), before);
    assert_eq!(s.logical_path(), Some(f.file.as_path()));
    s.stop_all();
}

/// The first Save and what waits for it, on the window's own session owner and
/// save worker: a refused, cancelled or private-folder Save As publishes nothing,
/// keeps the session untitled and continues nothing; only a published one names
/// the document, clears it, and hands back the continuation exactly once.
#[test]
fn the_first_save_names_the_document_and_only_then_continues_once() {
    fn failure() -> SaveFailure {
        SaveFailure {
            kind: SaveFailureKind::Failed,
            error: CadError::io("saving", "late"),
        }
    }
    let sessions_root = tempfile::tempdir().expect("sessions");
    let user = tempfile::tempdir().expect("user");
    let session = DocumentSession::create_document_in(
        sessions_root.path(),
        HistoryLimits::default(),
        NewDocument::SamplePlate(PlateSize::DEFAULT),
        ferritecad_occt::OcctKernel::new,
        &OperationContext::default(),
    )
    .expect("kernel-free creation");
    let mut s = Sessions::default();
    s.adopt(session);
    assert!(s.untitled() && s.dirty() && s.can_save() && s.can_save_as());
    assert_eq!(s.title(), "*Untitled — FerriteCAD");
    let private = s.private_directory().expect("private").to_path_buf();
    assert_eq!(shown_as(&private.join("v0.fcad")), "Untitled");
    assert_eq!(
        s.suggestion().expect("suggestion"),
        PathBuf::from(".").join("Untitled")
    );
    // The question replacing it asks: Save, Discard or Cancel, never "go".
    assert_eq!(s.replacing(None), Replace::Stay);
    assert_eq!(s.replacing(Some(UnsavedChoice::Cancel)), Replace::Stay);
    assert_eq!(
        s.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    assert_eq!(s.replacing(Some(UnsavedChoice::Save)), Replace::AfterSave);
    assert!(s.has_session(), "Discard alone drops nothing");

    let run = |s: &mut Sessions, target: SaveTarget, cancel: bool| {
        let (tx, rx) = mpsc::channel();
        let g = s
            .begin_save(target, Some(Continuation::Create), |p, _, c| {
                if cancel {
                    c.cancel();
                }
                spawn_save(p, c.clone(), move |v| tx.send(v).expect("save"))
            })
            .expect("started");
        let report = s
            .finish_save(g, rx.recv().expect("answer"))
            .expect("report");
        // The same answer again is not a second continuation.
        assert_eq!(s.finish_save(g, Err(failure())), None);
        report
    };
    let fresh = state(&s);

    let taken = user.path().join("taken.fcad");
    std::fs::write(&taken, b"theirs").expect("occupied");
    for (target, cancel) in [
        (SaveTarget::As(taken.clone()), false),
        (SaveTarget::As(private.join("inside.fcad")), false),
        (SaveTarget::InPlace, false),
        (SaveTarget::As(user.path().join("cancelled.fcad")), true),
    ] {
        let report = run(&mut s, target, cancel);
        assert!(
            !report.published && report.continuation.is_none(),
            "{report:?}"
        );
        assert!(s.untitled() && s.dirty());
        assert_eq!(state(&s), fresh);
        assert!(
            !s.status.contains(&private.display().to_string()),
            "{}",
            s.status
        );
    }
    assert_eq!(std::fs::read(&taken).expect("theirs"), b"theirs");
    assert!(!user.path().join("cancelled.fcad").exists());

    let file = user.path().join("first.fcad");
    let report = run(&mut s, SaveTarget::As(file.clone()), false);
    assert_eq!(
        report,
        SaveReport {
            published: true,
            continuation: Some(Continuation::Create),
        }
    );
    assert_eq!(s.logical_path(), Some(file.as_path()));
    assert!(!s.untitled() && !s.dirty());
    assert_eq!(s.title(), "first.fcad — FerriteCAD");
    assert_eq!(
        shown_as(&private.join("v0.fcad")),
        file.display().to_string()
    );
    assert_eq!(s.replacing(None), Replace::Go);
    s.stop_all();
}

// --- the real window's outputs ---------------------------------------------------

/// What the window run leaves in its root (see the verification recipe).
const WINDOW_OUTPUTS: [&str; 9] = [
    "unsaved.stl",
    "unsaved.fbx",
    "first-save.fcad",
    "undo.stl",
    "after-guard.fcad",
    "plate.fcad",
    "annulus.stl",
    "annulus.fbx",
    "empty.fcad",
];

/// FBX text with every UUID replaced by the order it first appears in: equal for
/// two exports exactly when they agree in everything but a bijection of UUIDs.
fn fbx_canonical(fbx: &[u8]) -> String {
    let text = String::from_utf8(fbx.to_vec()).expect("ASCII FBX");
    let bytes = text.as_bytes();
    let is_uuid = |s: &[u8]| {
        s.len() == 36
            && s.iter().enumerate().all(|(i, b)| match i {
                8 | 13 | 18 | 23 => *b == b'-',
                _ => b.is_ascii_hexdigit(),
            })
    };
    let mut seen: Vec<String> = Vec::new();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 36 <= bytes.len() && is_uuid(&bytes[i..i + 36]) {
            let id = text[i..i + 36].to_owned();
            let n = seen.iter().position(|s| *s == id).unwrap_or_else(|| {
                seen.push(id);
                seen.len() - 1
            });
            out.push_str(&format!("<uuid{n}>"));
            i += 36;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// §30L: the actual window files against a temporary command-line chain. A
/// missing output is refused before any peer job.
fn compare_gui(root: &Path) {
    for name in WINDOW_OUTPUTS {
        assert!(root.join(name).is_file(), "missing real GUI output: {name}");
    }
    let file = |name: &str| std::fs::read(root.join(name)).expect(name);
    // The generator's pristine copy of the name the window was refused.
    assert_eq!(
        file("occupied.fcad"),
        file("inputs/occupied.fcad"),
        "the occupied file was written"
    );
    assert_eq!(
        file("plate.fcad"),
        file("after-guard.fcad"),
        "the guarded Save is the file left behind"
    );
    let tmp = tempfile::tempdir().expect("peer root");
    let peer = tmp.path();
    let base = peer.join("base.fcad");
    cli(&[
        "create".as_ref(),
        base.as_os_str(),
        "--sample".as_ref(),
        "--size".as_ref(),
        "83".as_ref(),
        "47".as_ref(),
        "13".as_ref(),
    ]);
    let reading = ferritecad_jobs::read_extrude_source(&base).expect("base");
    let edited = peer.join("edited.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        base.as_os_str(),
        "--feature".as_ref(),
        reading.features[0].feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        reading.version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        "21.5".as_ref(),
        "-o".as_ref(),
        edited.as_os_str(),
    ]);

    // The first Save is the edited plate, under a bijection of its new identities.
    let first = root.join("first-save.fcad");
    let map = pair_independent(&first, &edited);
    assert_eq!(
        independent_cells(&first, &map),
        independent_cells(&edited, &BTreeMap::new()),
        "the first save is the edited plate"
    );
    // The guarded Save is the same document after Undo: the same identities in
    // the same places, and the unedited plate.
    let after = root.join("after-guard.fcad");
    assert_eq!(
        identities(&after),
        identities(&first),
        "the guarded save is the same document"
    );
    assert_eq!(
        independent_cells(&after, &map),
        independent_cells(&base, &BTreeMap::new()),
        "the guarded save is the plate after Undo"
    );

    let (stl, fbx) = peer_bytes(&edited, peer, "edited");
    assert_eq!(file("unsaved.stl"), stl, "unsaved STL against the CLI");
    assert_eq!(
        fbx_mapped(&file("unsaved.fbx"), &map),
        String::from_utf8(fbx).expect("ASCII"),
        "unsaved FBX against the CLI"
    );
    assert_eq!(
        file("undo.stl"),
        peer_bytes(&base, peer, "base").0,
        "Undo STL against the CLI"
    );

    let annulus = cli_create(
        &variants()
            .into_iter()
            .find(|v| v.name == "annulus")
            .expect("annulus"),
        peer,
    );
    let (stl, fbx) = peer_bytes(&annulus, peer, "annulus");
    assert_eq!(file("annulus.stl"), stl, "annulus STL against the CLI");
    assert_eq!(
        fbx_canonical(&file("annulus.fbx")),
        fbx_canonical(&fbx),
        "annulus FBX against the CLI under a bijection of UUIDs"
    );

    let empty = cli_create(
        &variants()
            .into_iter()
            .find(|v| v.name == "empty")
            .expect("empty"),
        peer,
    );
    let window_empty = root.join("empty.fcad");
    let map = pair_independent(&window_empty, &empty);
    assert_eq!(
        independent_cells(&window_empty, &map),
        independent_cells(&empty, &BTreeMap::new()),
        "the saved Empty document"
    );
}

/// The comparator on a copy of `root` with one fact broken must refuse, for the
/// reason `why` names.
fn control(root: &Path, name: &str, why: &str, breaks: &dyn Fn(&Path)) {
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
        "control {name} refused for another reason"
    );
}

/// The comparator's positive run and its seven negative controls.
fn compare_with_controls(root: &Path) {
    compare_gui(root);
    let before = cli_runs();
    control(root, "missing", "missing real GUI output: undo.stl", &|r| {
        std::fs::remove_file(r.join("undo.stl")).expect("remove")
    });
    assert_eq!(
        cli_runs(),
        before,
        "a peer job ran before the missing output was refused"
    );
    let file = |r: &Path, name: &str| std::fs::read(r.join(name)).expect(name);
    control(
        root,
        "occupied written",
        "the occupied file was written",
        &|r| std::fs::write(r.join("occupied.fcad"), b"replaced").expect("write"),
    );
    control(
        root,
        "old document as first save",
        "the first save is the edited plate",
        &|r| std::fs::write(r.join("first-save.fcad"), file(r, "after-guard.fcad")).expect("w"),
    );
    control(
        root,
        "another document as the guarded save",
        "the guarded save is the same document",
        &|r| {
            for name in ["after-guard.fcad", "plate.fcad"] {
                std::fs::write(r.join(name), file(r, "empty.fcad")).expect("w");
            }
        },
    );
    control(
        root,
        "export of the old version",
        "unsaved STL against the CLI",
        &|r| std::fs::write(r.join("unsaved.stl"), file(r, "undo.stl")).expect("w"),
    );
    control(
        root,
        "the plate's FBX as the annulus",
        "annulus FBX against the CLI",
        &|r| std::fs::write(r.join("annulus.fbx"), file(r, "unsaved.fbx")).expect("w"),
    );
    // A reference that names the wrong owner is a wrong reference, whatever its UUID.
    control(root, "wrong ref owner", "would pair with", &|r| {
        let first = r.join("first-save.fcad");
        let db = rusqlite::Connection::open(&first).expect("db");
        let objects: Vec<Vec<u8>> = db
            .prepare("SELECT id FROM objects ORDER BY rowid")
            .expect("ids")
            .query_map([], |row| row.get(0))
            .expect("rows")
            .collect::<std::result::Result<_, _>>()
            .expect("ids");
        db.execute(
            "UPDATE topology_refs SET owner_id=?1 WHERE rowid=(SELECT min(rowid) FROM topology_refs)",
            rusqlite::params![objects[0]],
        )
        .expect("wrong owner");
    });
}

/// The window recipe run through the window's own create worker, session owner
/// and save worker, its files laid out as the window leaves them, then the
/// comparator and its controls. A self-check of the comparator, not window
/// evidence.
#[test]
fn native_window_scenario_on_session_files_passes_the_comparator_and_its_controls() {
    if !native() {
        return;
    }
    let work = tempfile::tempdir().expect("work");
    let layout = work.path().join("layout");
    std::fs::create_dir_all(&layout).expect("layout");
    let sessions_root = tempfile::tempdir().expect("sessions");
    let occupied = layout.join("occupied.fcad");
    cli(&["create".as_ref(), occupied.as_os_str()]);
    std::fs::create_dir_all(layout.join("inputs")).expect("inputs");
    std::fs::copy(&occupied, layout.join("inputs/occupied.fcad")).expect("pristine");
    let put = |name: &str, bytes: &[u8]| std::fs::write(layout.join(name), bytes).expect(name);

    let mut s = Sessions::default();
    let plate = variants().swap_remove(1).content;
    create_into(&mut s, sessions_root.path(), plate);
    let created = s.export_path().expect("created");
    apply_native(&mut s, first_extrude(&created), 21.5);
    let alias = s.suggestion().expect("suggestion");
    let (stl, fbx) = export_bytes(&s.export_path().expect("edited"), &alias, work.path(), "u");
    put("unsaved.stl", &stl);
    put("unsaved.fbx", &fbx);
    // The occupied name is refused and changes nothing.
    let (tx, rx) = mpsc::channel();
    let g = s
        .begin_save(SaveTarget::As(occupied.clone()), None, |p, _, c| {
            spawn_save(p, c.clone(), move |v| tx.send(v).expect("save"))
        })
        .expect("started");
    assert!(
        !s.finish_save(g, rx.recv().expect("answer"))
            .expect("report")
            .published
    );
    let plate_file = layout.join("plate.fcad");
    saved(&mut s, SaveTarget::As(plate_file.clone()));
    put(
        "first-save.fcad",
        &std::fs::read(&plate_file).expect("first"),
    );
    move_native(&mut s, true);
    put(
        "undo.stl",
        &export_bytes(
            &s.export_path().expect("undone"),
            &plate_file,
            work.path(),
            "d",
        )
        .0,
    );
    move_native(&mut s, false);
    move_native(&mut s, true);
    // The guarded Save before the annulus replaces it: in place.
    saved(&mut s, SaveTarget::InPlace);
    put(
        "after-guard.fcad",
        &std::fs::read(&plate_file).expect("guarded"),
    );
    let annulus = variants().swap_remove(4).content;
    create_into(&mut s, sessions_root.path(), annulus);
    let alias = s.suggestion().expect("suggestion");
    let (stl, fbx) = export_bytes(&s.export_path().expect("annulus"), &alias, work.path(), "a");
    put("annulus.stl", &stl);
    put("annulus.fbx", &fbx);
    // Discard: the untitled annulus is replaced by an Empty document.
    assert_eq!(
        s.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    create_into(&mut s, sessions_root.path(), NewDocument::Empty);
    saved(&mut s, SaveTarget::As(layout.join("empty.fcad")));
    s.stop_all();
    compare_with_controls(&layout);
}

#[test]
fn native_compare_real_new_document_gui_artifacts_with_negative_controls() {
    let Some(root) = std::env::var_os("FCAD_30L_GUI_DIR") else {
        eprintln!("skipped: requires real GUI artifacts");
        return;
    };
    assert!(native());
    compare_with_controls(Path::new(&root));
    println!("FCAD_30L_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true");
}
