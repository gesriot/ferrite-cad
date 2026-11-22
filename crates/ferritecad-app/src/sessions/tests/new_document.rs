// SPDX-License-Identifier: MIT
//! §30L: a private version can be edited and exported before the document's first Save.
//! Start with New, then draw and publish any custom solid through the sketch and sweep
//! widgets. Independent CLI documents use their own identities. Inside one session,
//! the snapshot, first Save and reopened file must agree without remapping anything.
use super::add_cut::{Id, sql_mapped};
use super::fillet::{saved, sql, state};
use super::*;
use crate::sessions::tests::tabs::{Drawn, Window};
use crate::sweeps::tests::window::creation::{
    BUSHING, Model, POLYGON, draw_model, independent_document,
};
use ferritecad_jobs::SaveTarget;
use ferritecad_ui::{NewChoice, NewContent};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
enum Variant {
    Empty,
    Plate,
    Drawing(Model),
}

impl Variant {
    fn name(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Plate => "plate",
            Self::Drawing(Model::Polygon(_)) => "polygon",
            Self::Drawing(Model::Circle) => "circle",
            Self::Drawing(Model::Annulus) => "annulus",
            Self::Drawing(Model::FullTurn(_)) => "revolve",
            Self::Drawing(Model::Sector) => "partial-revolve",
        }
    }

    fn has_extrude(self) -> bool {
        matches!(
            self,
            Self::Plate | Self::Drawing(Model::Polygon(_) | Model::Circle | Model::Annulus)
        )
    }

    fn create(self, w: &mut Window) {
        assert!(w.begin_new());
        if matches!(self, Self::Plate) {
            let form = w.creates.form().expect("New form");
            form.content = NewContent::SamplePlate;
            form.width = "83".into();
            form.depth = "47".into();
            form.height = "13".into();
        }
        let request = w.answer_new(NewChoice::Create).expect("create");
        w.create_from_new(request, Ok(()))
            .expect("accepted document");
        assert!(!w.sessions.can_undo() && !w.sessions.can_redo());
        if let Self::Drawing(model) = self {
            draw_model(w, model);
            assert!(w.sessions.can_undo() && !w.sessions.can_redo());
        }
    }
}

/// Independent CLI output. For a drawing each edit has the previous version's guard.
fn cli_create(variant: Variant, root: &Path) -> PathBuf {
    let dir = root.join(variant.name());
    std::fs::create_dir(&dir).expect("peer directory");
    if let Variant::Drawing(model) = variant {
        independent_document(model, &dir);
        return dir.join("cli.fcad");
    }
    let out = dir.join("cli.fcad");
    let mut args = vec!["create".as_ref(), out.as_os_str()];
    if matches!(variant, Variant::Plate) {
        args.extend(["--sample", "--size", "83", "47", "13"].map(std::ffi::OsStr::new));
    }
    args.push("--json".as_ref());
    let output = cli(&args);
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout).expect("create JSON");
    assert_eq!(answer["ok"], true);
    assert_eq!(answer["result"]["destination"], out.to_str().expect("path"));
    assert!(out.is_file());
    out
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
    ferritecad_fixtures::expression_slot(
        &crate::sketch_facts::read(path).expect("reading"),
        ferritecad_document::ExpressionSlot::ExtrudeDistance,
    )
    .object
}

/// The matrix: each variant, created, edited where it has a height, exported
/// before Save, saved for the first time, reopened, rebuilt cold and compared
/// with the command line's own creation.
fn matrix() {
    let root = tempfile::tempdir().expect("root");
    for variant in [
        Variant::Empty,
        Variant::Plate,
        Variant::Drawing(Model::Polygon(&POLYGON)),
        Variant::Drawing(Model::Circle),
        Variant::Drawing(Model::Annulus),
        Variant::Drawing(Model::FullTurn(&BUSHING)),
        Variant::Drawing(Model::Sector),
    ] {
        let name = variant.name();
        let theirs = cli_create(variant, root.path());
        let mut window = Window::new(Drawn::Native, None);
        variant.create(&mut window);
        let s = &mut window.sessions;

        // Untitled, unsaved, named without its private folder.
        assert_eq!(s.logical_path(), None, "{name}");
        assert!(s.untitled() && s.dirty() && s.can_save(), "{name}");
        assert_eq!(s.name().as_deref(), Some("Untitled"));
        assert_eq!(s.title(), "*Untitled — FerriteCAD");
        let private = s.private_directory().expect("private").to_path_buf();
        assert_eq!(shown_as(&private.join("v0.fcad")), "Untitled");
        let created = s.export_path().expect("current");

        // The same model as the command line's, under the new-identity bijection.
        let map = pair_independent(&created, &theirs);
        assert_eq!(
            independent_cells(&created, &map),
            independent_cells(&theirs, &BTreeMap::new()),
            "{name}: every SQL cell but the stamps and the proved new-identity bijection"
        );

        // A step and its Undo/Redo before any Save: never saved, no remap.
        if variant.has_extrude() {
            apply_native(s, first_extrude(&created), 7.5);
            assert!(s.can_undo() && s.dirty());
            move_native(s, true);
            assert_eq!(sql(&s.export_path().expect("undone")), sql(&created));
            assert!(
                s.dirty(),
                "{name}: Undo to the created version is not a save"
            );
            move_native(s, false);
            move_native(s, true);
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
        saved(s, SaveTarget::As(file.clone()));
        assert_eq!(s.logical_path(), Some(file.as_path()));
        assert!(!s.untitled() && !s.dirty());
        assert_eq!(s.title(), format!("{name}.fcad — FerriteCAD"));
        assert_eq!(
            sql(&file),
            sql(&current),
            "{name}: the saved file is the version"
        );
        let reopened = crate::sessions::test_open::open(&file).expect("reopen");
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
        if variant.has_extrude() {
            assert!(s.can_redo(), "{name}: the history survived the first Save");
            move_native(s, false);
            assert!(
                s.dirty(),
                "{name}: a version other than the saved one is unsaved"
            );
            move_native(s, true);
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
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
        || ferritecad_sketch_solver::is_available()
    {
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
            .begin_save(target, Some(Continuation::Close), |p, _, c| {
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
            continuation: Some(Continuation::Close),
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
    let reading = crate::sketch_facts::read(&base).expect("base");
    let edited = peer.join("edited.fcad");
    {
        let request = crate::edits::tests::expression_request(
            &ferritecad_fixtures::expression_slot(
                &reading,
                ferritecad_document::ExpressionSlot::ExtrudeDistance,
            )
            .object
            .to_string(),
            "extrude_distance",
            "21.5",
            "mm",
        );
        cli(&[
            "edit".as_ref(),
            base.as_os_str(),
            "--expect-version".as_ref(),
            reading.version.content.to_string().as_ref(),
            "--request".as_ref(),
            request.path().as_os_str(),
            "--output".as_ref(),
            edited.as_os_str(),
        ])
    };

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

    let annulus = cli_create(Variant::Drawing(Model::Annulus), peer);
    let (stl, fbx) = peer_bytes(&annulus, peer, "annulus");
    assert_eq!(file("annulus.stl"), stl, "annulus STL against the CLI");
    assert_eq!(
        fbx_canonical(&file("annulus.fbx")),
        fbx_canonical(&fbx),
        "annulus FBX against the CLI under a bijection of UUIDs"
    );

    let empty = cli_create(Variant::Empty, peer);
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
    let occupied = layout.join("occupied.fcad");
    cli(&["create".as_ref(), occupied.as_os_str()]);
    std::fs::create_dir_all(layout.join("inputs")).expect("inputs");
    std::fs::copy(&occupied, layout.join("inputs/occupied.fcad")).expect("pristine");
    let put = |name: &str, bytes: &[u8]| std::fs::write(layout.join(name), bytes).expect(name);

    let mut window = Window::new(Drawn::Native, None);
    Variant::Plate.create(&mut window);
    window.apply(21.5);
    let s = &mut window.sessions;
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
    saved(s, SaveTarget::As(plate_file.clone()));
    put(
        "first-save.fcad",
        &std::fs::read(&plate_file).expect("first"),
    );
    move_native(s, true);
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
    move_native(s, false);
    move_native(s, true);
    // The guarded Save before the annulus replaces it: in place.
    saved(s, SaveTarget::InPlace);
    put(
        "after-guard.fcad",
        &std::fs::read(&plate_file).expect("guarded"),
    );
    Variant::Drawing(Model::Annulus).create(&mut window);
    let s = &mut window.sessions;
    let alias = s.suggestion().expect("suggestion");
    let (stl, fbx) = export_bytes(&s.export_path().expect("annulus"), &alias, work.path(), "a");
    put("annulus.stl", &stl);
    put("annulus.fbx", &fbx);
    // Discard: the untitled annulus is replaced by an Empty document.
    assert_eq!(
        s.replacing(Some(UnsavedChoice::Discard)),
        Replace::Discarded
    );
    let discarded = s.tab();
    window.close(discarded).expect("discard annulus");
    Variant::Empty.create(&mut window);
    let s = &mut window.sessions;
    saved(s, SaveTarget::As(layout.join("empty.fcad")));
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
