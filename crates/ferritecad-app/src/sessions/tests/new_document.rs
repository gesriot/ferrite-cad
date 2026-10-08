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
            "an identity pairs with two"
        );
        assert_eq!(
            *back.entry(*y).or_insert(*x),
            *x,
            "two identities pair with one"
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
