// SPDX-License-Identifier: MIT
//! §30I: a new circular Cut added through the session's real worker, compared with
//! the shipped CLI. The window and the CLI each mint their own UUIDs for what is new;
//! only a proved bijection of genuinely new identities is applied, never to an old
//! UUID, to a whole payload or to an order. Undo/Redo is compared with no remap.
use super::cut::{apply, cold, mesh, peer, refs, saved};
use super::*;
use crate::cuts::tests::add_session::typed_add;
use crate::cuts::tests::session_apply::{draft_state, fixture, selected, typed_cut};
use ferritecad_document::{CircularCut, CutExtent, ObjectPayload};
use ferritecad_jobs::CircularCutRequest;
use std::collections::BTreeMap;

type Id = [u8; 16];

fn start(s: &mut Sessions, r: &CircularCutRequest) -> (u64, mpsc::Receiver<Result<ProducedStep>>) {
    let (tx, rx) = mpsc::channel();
    let g = s
        .begin_apply(|t, _, c| {
            spawn_add_cut(t, r.body, r.cut, r.expected, c.clone(), move |v| {
                tx.send(v).expect("deliver")
            })
        })
        .expect("start");
    (g, rx)
}
/// The window's own Add: the worker, the picture, the version becoming current.
fn add(s: &mut Sessions, r: &CircularCutRequest) -> Edited {
    let (g, rx) = start(s, r);
    let edited = s.finish_apply(
        g,
        rx.recv_timeout(std::time::Duration::from_secs(120))
            .expect("edit"),
    );
    if let Edited::Show(path) = &edited {
        let (tx, rx) = mpsc::channel();
        let c = s.scene_token(g).expect("token");
        let worker = spawn_scene(path.clone(), c, move |v| tx.send(v).expect("scene"));
        assert!(s.attach_scene(g, worker));
        rx.recv_timeout(std::time::Duration::from_secs(120))
            .expect("scene")
            .expect("prepared scene");
        s.bind(Bind::Staged).expect("bind");
        assert!(s.finish_scene(g, Ok(())));
    }
    edited
}
/// A refused Add changes nothing the session owns and leaves no file.
fn refuse(s: &mut Sessions, r: &CircularCutRequest) -> String {
    let state = (
        s.export_path(),
        s.dirty(),
        s.can_undo(),
        s.can_redo(),
        s.session_saved_version(),
        private_files(s),
    );
    assert_eq!(add(s, r), Edited::Failed, "{}", s.status);
    assert_eq!(
        (
            s.export_path(),
            s.dirty(),
            s.can_undo(),
            s.can_redo(),
            s.session_saved_version(),
            private_files(s)
        ),
        state
    );
    s.status.clone()
}
/// `cut-circular-copy` on `source`, naming the Body by its UUID there.
fn peer_add(source: &Path, body: ObjectId, cut: &CircularCut, out: &Path) {
    let file = out.with_extension("json");
    let extent = match cut.extent {
        CutExtent::ThroughAll => r#"{"kind":"through_all"}"#.to_owned(),
        CutExtent::Blind { depth_mm } => format!(r#"{{"kind":"blind","depth_mm":{depth_mm}}}"#),
    };
    std::fs::write(
        &file,
        format!(
            r#"{{"request_version":2,"center_mm":[{},{}],"radius_mm":{},"extent":{extent}}}"#,
            cut.center_mm[0], cut.center_mm[1], cut.radius_mm
        ),
    )
    .expect("request");
    cli(&[
        "cut-circular-copy".as_ref(),
        source.as_os_str(),
        "--body".as_ref(),
        body.to_string().as_ref(),
        "--expect-version".as_ref(),
        read_extrude_source(source)
            .expect("reading")
            .version
            .content
            .to_string()
            .as_ref(),
        "--request".as_ref(),
        file.as_os_str(),
        "-o".as_ref(),
        out.as_os_str(),
        "--json".as_ref(),
    ]);
}

/// Replaces a mapped UUID: a blob equal to it, and every CBOR byte string
/// (`0x50` + 16 bytes) of it inside a blob, nested payloads included.
fn remap(blob: &[u8], map: &BTreeMap<Id, Id>) -> Vec<u8> {
    if let Ok(id) = <Id>::try_from(blob)
        && let Some(to) = map.get(&id)
    {
        return to.to_vec();
    }
    let mut out = blob.to_vec();
    for (from, to) in map {
        let needle: Vec<u8> = std::iter::once(0x50).chain(from.iter().copied()).collect();
        let mut i = 0;
        while i + needle.len() <= out.len() {
            if out[i..i + needle.len()] == needle[..] {
                out[i + 1..i + 17].copy_from_slice(to);
                i += needle.len();
            } else {
                i += 1;
            }
        }
    }
    out
}

/// Every row of `table` as column → text, rowid included where SQLite exposes it,
/// `meta.modified_at` set aside, mapped UUIDs replaced. A row with a payload and
/// its hash has the raw hash checked first and is shown with the hash of the
/// mapped payload, so a mismatch anywhere else in the payload still shows.
fn rows(
    db: &rusqlite::Connection,
    table: &str,
    map: &BTreeMap<Id, Id>,
) -> Vec<BTreeMap<String, String>> {
    let quoted = table.replace('"', "\"\"");
    let mut q = db
        .prepare(&format!("SELECT rowid,* FROM \"{quoted}\""))
        .or_else(|_| db.prepare(&format!("SELECT * FROM \"{quoted}\"")))
        .expect("table");
    let columns: Vec<String> = q.column_names().into_iter().map(str::to_owned).collect();
    q.query_map([], |r| {
        let mut raw = Vec::new();
        for i in 0..columns.len() {
            raw.push(r.get::<_, rusqlite::types::Value>(i)?);
        }
        Ok(raw)
    })
    .expect("rows")
    .collect::<std::result::Result<Vec<_>, _>>()
    .expect("rows")
    .into_iter()
    .map(|raw| {
        let at = |name: &str| columns.iter().position(|c| c == name);
        let payload = match (at("payload"), at("payload_hash")) {
            (Some(p), Some(h)) => match (&raw[p], &raw[h]) {
                (rusqlite::types::Value::Blob(p), hash) => {
                    if let rusqlite::types::Value::Blob(hash) = hash {
                        assert_eq!(
                            ferritecad_types::ContentHash::of_bytes(p).as_bytes(),
                            &hash[..],
                            "raw payload hash in {table}"
                        );
                    }
                    Some((remap(p, map), !matches!(hash, rusqlite::types::Value::Null)))
                }
                _ => None,
            },
            _ => None,
        };
        let mut row = BTreeMap::new();
        for (i, column) in columns.iter().enumerate() {
            if table == "meta" && column == "modified_at" {
                continue;
            }
            let text = match (column.as_str(), &raw[i], &payload) {
                ("payload", _, Some((p, _))) => format!("{:?}", p),
                ("payload_hash", _, Some((p, true))) => format!(
                    "{:?}",
                    ferritecad_types::ContentHash::of_bytes(p).as_bytes()
                ),
                (_, rusqlite::types::Value::Blob(b), _) => format!("{:?}", remap(b, map)),
                (_, other, _) => format!("{other:?}"),
            };
            row.insert(column.clone(), text);
        }
        row
    })
    .collect()
}

/// Every SQL cell of every table under the map (see `rows`), rows sorted.
pub(super) fn sql_mapped(path: &Path, map: &BTreeMap<Id, Id>) -> BTreeMap<String, Vec<String>> {
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
    tables
        .into_iter()
        .map(|table| {
            let mut all: Vec<String> = rows(&db, &table, map)
                .into_iter()
                .map(|row| row.iter().map(|(c, v)| format!("{c}={v};")).collect())
                .collect();
            all.sort();
            (table, all)
        })
        .collect()
}

fn ids(path: &Path) -> Vec<(ObjectId, ObjectPayload)> {
    Document::open_read_only(path)
        .expect("document")
        .objects()
        .expect("objects")
        .into_iter()
        .map(|o| (o.id, o.payload))
        .collect()
}

/// Pairs what is new in `ours` (after `before`) with what is new in `theirs`
/// (after `theirs_before`), extending `map`: new objects by payload type (exactly
/// one of each), the new Sketch's curves in stored order, and new references by
/// every other cell once the objects and curves are mapped. Nothing old is mapped.
fn pair_new(
    map: &mut BTreeMap<Id, Id>,
    before: &Path,
    ours: &Path,
    theirs_before: &Path,
    theirs: &Path,
) {
    let new = |before: &Path, after: &Path| {
        let old: Vec<_> = ids(before).into_iter().map(|o| o.0).collect();
        ids(after)
            .into_iter()
            .filter(|o| !old.contains(&o.0))
            .collect::<Vec<_>>()
    };
    let earlier: Vec<Id> = map.keys().copied().collect();
    let (a, b) = (new(before, ours), new(theirs_before, theirs));
    assert_eq!(
        a.len(),
        2,
        "one Add makes one Cut feature and one tool Sketch"
    );
    assert_eq!(b.len(), 2);
    for (id, payload) in &a {
        let peer: Vec<_> = b
            .iter()
            .filter(|o| o.1.type_name() == payload.type_name())
            .collect();
        assert_eq!(peer.len(), 1, "one new {}", payload.type_name());
        assert!(map.insert(id.to_bytes(), peer[0].0.to_bytes()).is_none());
        if let (ObjectPayload::Sketch(x), ObjectPayload::Sketch(y)) = (payload, &peer[0].1) {
            assert_eq!(x.curves.len(), y.curves.len());
            for (u, v) in x.curves.iter().zip(&y.curves) {
                assert!(map.insert(u.id.to_bytes(), v.id.to_bytes()).is_none());
            }
        }
    }
    // New references: equal in every cell but their own id once mapped.
    let row_of = |path: &Path, map: &BTreeMap<Id, Id>| -> BTreeMap<Id, String> {
        let db =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .expect("db");
        rows(&db, "topology_refs", map)
            .into_iter()
            .map(|row| {
                // A new ref's own id is not in the map yet: it is shown raw.
                let id: Vec<u8> = row["id"]
                    .trim_matches(|c| c == '[' || c == ']')
                    .split(", ")
                    .map(|b| b.parse().expect("byte"))
                    .collect();
                let rest = row
                    .iter()
                    .filter(|(c, _)| *c != "id" && *c != "rowid")
                    .map(|(c, v)| format!("{c}={v};"))
                    .collect::<String>();
                (<Id>::try_from(&id[..]).expect("16 bytes"), rest)
            })
            .collect()
    };
    let old_ours: Vec<_> = refs(before).iter().map(|r| r.id.to_bytes()).collect();
    let old_theirs: Vec<_> = refs(theirs_before)
        .iter()
        .map(|r| r.id.to_bytes())
        .collect();
    // Old references paired by an earlier Add are shown under their peer id.
    let shown_old: Vec<_> = old_ours
        .iter()
        .map(|id| map.get(id).copied().unwrap_or(*id))
        .collect();
    let (ra, rb) = (row_of(ours, map), row_of(theirs, &BTreeMap::new()));
    let mut fresh = 0;
    for (id, rest) in ra.iter().filter(|(id, _)| !shown_old.contains(id)) {
        let found: Vec<_> = rb
            .iter()
            .filter(|(pid, prest)| !old_theirs.contains(pid) && *prest == rest)
            .collect();
        assert_eq!(found.len(), 1, "a new ref pairs with exactly one");
        assert!(map.insert(*id, *found[0].0).is_none());
        fresh += 1;
    }
    assert!(fresh > 0, "an Add names its new faces");
    assert_eq!(
        fresh,
        rb.keys().filter(|id| !old_theirs.contains(id)).count(),
        "the same number of new references"
    );
    for key in map.keys().filter(|k| !earlier.contains(k)) {
        assert!(
            !old_ours.contains(key) && !ids(before).iter().any(|o| &o.0.to_bytes() == key),
            "an old identity was mapped"
        );
    }
}

fn same_model(ours: &Path, theirs: &Path, map: &BTreeMap<Id, Id>, why: &str) {
    assert_eq!(
        sql_mapped(ours, map),
        sql_mapped(theirs, &BTreeMap::new()),
        "{why}: every SQL cell but modified_at and the proved new-UUID bijection"
    );
}

/// Old identities and cells survive an Add: every old object row is unchanged
/// except the Body (its tip) and nothing else; every old reference is unchanged.
fn assert_allowlist(before: &Path, after: &Path, body: ObjectId, added: ObjectId) {
    let old: BTreeMap<_, _> = ids(before).into_iter().collect();
    let new: BTreeMap<_, _> = ids(after).into_iter().collect();
    for (id, payload) in &old {
        let now = new.get(id).expect("an old object vanished");
        if *id == body {
            let (ObjectPayload::Body(was), ObjectPayload::Body(is)) = (payload, now) else {
                panic!("Body")
            };
            assert_ne!(was.tip_feature, Some(added));
            assert_eq!(is.tip_feature, Some(added), "the Body tip is the new Cut");
        } else {
            assert_eq!(payload, now, "old object {id} changed");
        }
    }
    let ObjectPayload::Extrude(cut) = &new[&added] else {
        panic!("the new Cut is an Extrude")
    };
    let ObjectPayload::Body(was) = &old[&body] else {
        panic!("Body")
    };
    assert_eq!(
        cut.previous, was.tip_feature,
        "its predecessor is the old tip"
    );
    // A Cut names the result it modifies; the Body names its tip (domain rule).
    assert_eq!(cut.target_body, None);
    assert_eq!(cut.operation, ferritecad_document::SolidOperation::Cut);
    for r in refs(before) {
        assert!(refs(after).contains(&r), "old ref {} changed", r.id);
    }
}

fn added_feature(before: &Path, after: &Path) -> ObjectId {
    let old: Vec<_> = ids(before).into_iter().map(|o| o.0).collect();
    ids(after)
        .into_iter()
        .find(|o| !old.contains(&o.0) && matches!(o.1, ObjectPayload::Extrude(_)))
        .expect("new Cut")
        .0
}

fn cut_ids(path: &Path) -> Vec<ObjectId> {
    read_extrude_source(path)
        .expect("reading")
        .cut_features
        .iter()
        .filter_map(|c| c.saved.as_ref().map(|s| s.feature))
        .collect()
}

const SECOND: (&str, &str, &str, Option<&str>) = ("66.5", "10.75", "1.875", None);

fn gate(name: &str) {
    if !native() {
        return;
    }
    let (root, source, opened) = fixture(3);
    let original = std::fs::read(&source).expect("source");
    cold(&source);
    let body = opened.cut_bodies[0].body;
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    // Dirty base: height, then the left wall's two vertices.
    apply_native(&mut s, selected(&opened, 0).base_feature, 15.25);
    let peer_height = root.path().join("peer-height.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        source.as_os_str(),
        "--feature".as_ref(),
        selected(&opened, 0).base_feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        opened.version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        "15.25".as_ref(),
        "-o".as_ref(),
        peer_height.as_os_str(),
    ]);
    let current = s.export_path().expect("current");
    let reading = read_extrude_source(&current).expect("reading");
    let base = reading
        .sketches
        .iter()
        .find(|c| c.vertices.is_some())
        .expect("base");
    let mut v = ferritecad_jobs::EditSketchRequest {
        source: current,
        expected: reading.version,
        sketch: base.sketch,
        vertices: base.vertices.clone().expect("vertices"),
        destination: PathBuf::new(),
    };
    for p in &mut v.vertices {
        if p.start_mm[0] == -2.5 {
            p.start_mm[0] = -3.75;
        }
    }
    apply_sketch_native(&mut s, v.clone());
    let mut peer_before = root.path().join("peer-vertices.fcad");
    sketch_peer(&peer_height, &v, root.path(), &peer_before);
    let mut map = BTreeMap::new();
    same_model(&s.export_path().expect("base"), &peer_before, &map, "base");
    let before_cuts = s.export_path().expect("before Cuts");
    let mut accepted = vec![before_cuts.clone()];
    let mut added = Vec::new();

    // Two Adds through the form's own widgets: a Blind pocket, then ThroughAll.
    for (step, typed) in [("19.25", "30.5", "2.125", Some("4.875")), SECOND]
        .into_iter()
        .enumerate()
    {
        let current = s.export_path().expect("current");
        let reading = read_extrude_source(&current).expect("reading");
        let (mut form, r) = typed_add(&current, &reading, &s, typed);
        assert_eq!(r.body, body, "the Body is its saved UUID");
        assert_eq!(r.expected, reading.version);
        let independent = root.path().join(format!("peer-add-{step}.fcad"));
        peer_add(&peer_before, body, &r.cut, &independent);
        let exact = root.path().join(format!("exact-add-{step}.fcad"));
        peer_add(&current, body, &r.cut, &exact);
        assert!(matches!(add(&mut s, &r), Edited::Show(_)), "{}", s.status);
        form.finish_session_change();
        assert!(!form.active(), "the form outlived the picture it described");
        let applied = s.export_path().expect("added");
        assert!(s.dirty());
        assert_eq!(
            std::fs::read(&source).expect("source"),
            original,
            "Add wrote the file"
        );
        let feature = added_feature(&current, &applied);
        assert_allowlist(&current, &applied, body, feature);
        assert!(
            cut_ids(&applied).contains(&feature),
            "Edit cut offers the new Cut"
        );
        assert_eq!(
            selected(&read_extrude_source(&applied).expect("r"), 0)
                .tools
                .last()
                .map(|t| t.feature),
            Some(feature)
        );
        let mut exact_map = BTreeMap::new();
        pair_new(&mut exact_map, &current, &applied, &current, &exact);
        same_model(
            &applied,
            &exact,
            &exact_map,
            "Add against the CLI on the same snapshot",
        );
        pair_new(&mut map, &current, &applied, &peer_before, &independent);
        same_model(
            &applied,
            &independent,
            &map,
            "Add against the independent CLI chain",
        );
        accepted.push(applied);
        added.push(feature);
        peer_before = independent;
    }

    // Edit the second new Cut through §30F's own form and worker.
    let current = s.export_path().expect("current");
    let reading = read_extrude_source(&current).expect("reading");
    let index = selected(&reading, 0)
        .tools
        .iter()
        .position(|t| t.feature == added[1])
        .expect("new Cut is a tool");
    let (mut form, r) = typed_cut(&current, &reading, &s, index, "66.25", "2", None);
    assert_eq!(r.cut, added[1]);
    let mut theirs = r.clone();
    let back = |id: &Id| -> Id { map[id] };
    theirs.cut = ObjectId::from_bytes(back(&r.cut.to_bytes())).expect("id");
    theirs.edit.tool_curve =
        ferritecad_types::StableEntityId::from_bytes(back(&r.edit.tool_curve.to_bytes()))
            .expect("id");
    let edited_peer = root.path().join("peer-edit.fcad");
    peer(&peer_before, &theirs, root.path(), &edited_peer);
    assert!(matches!(apply(&mut s, &r), Edited::Show(_)), "{}", s.status);
    form.finish_session_change();
    let edited = s.export_path().expect("edited");
    same_model(&edited, &edited_peer, &map, "the new Cut edited");
    accepted.push(edited.clone());
    peer_before = edited_peer;

    // Unsaved exports are the accepted model, and the mesh is read here.
    let (stl, fbx) = export_bytes(&edited, &source, root.path(), "unsaved");
    assert_eq!(
        (stl.clone(), fbx.clone()),
        peer_bytes(&peer_before, root.path(), "peer-unsaved"),
        "unsaved STL/FBX against the CLI"
    );
    let exact = cold(&edited);
    let approx = mesh(&stl, &edited);

    // Undo/Redo within one session: no remap at all, and discovery follows.
    for expected in accepted.iter().rev().skip(1) {
        move_native(&mut s, true);
        let now = s.export_path().expect("Undo");
        assert_eq!(super::cut::sql(&now, &[]), super::cut::sql(expected, &[]));
    }
    let now = s.export_path().expect("before Cuts");
    assert!(
        added.iter().all(|f| !cut_ids(&now).contains(f)),
        "undone Cuts left discovery"
    );
    move_native(&mut s, true);
    move_native(&mut s, true);
    assert!(!s.dirty());
    move_native(&mut s, false);
    move_native(&mut s, false);
    for expected in accepted.iter().skip(1) {
        move_native(&mut s, false);
        assert_eq!(
            super::cut::sql(&s.export_path().expect("Redo"), &[]),
            super::cut::sql(expected, &[]),
            "Redo returns the very same UUIDs"
        );
    }
    assert!(
        added
            .iter()
            .all(|f| cut_ids(&s.export_path().expect("Redo")).contains(f))
    );
    assert_eq!(std::fs::read(&source).expect("source"), original);

    // Save, cold rebuild, reopen.
    saved(&mut s, SaveTarget::InPlace);
    same_model(&source, &peer_before, &map, "Save");
    cold(&source);
    let reopened = DocumentSession::open(&source).expect("reopen");
    assert!(!reopened.is_dirty());
    drop(reopened);
    if let Some(dir) = std::env::var_os("FCAD_ADD_CUT_SESSION_ARTIFACTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("artifacts");
        std::fs::write(dir.join(format!("{name}.stl")), &stl).expect("stl");
        std::fs::write(dir.join(format!("{name}.fbx")), &fbx).expect("fbx");
        std::fs::write(
            dir.join(format!("{name}.metrics.txt")),
            format!("analytical_mm3={exact:.9} stl_mm3={approx:.9}\n"),
        )
        .expect("metrics");
    }

    // From Undo of the edit: refusals keep everything, the draft included.
    move_native(&mut s, true);
    assert!(s.can_redo());
    let current = s.export_path().expect("current");
    let reading = read_extrude_source(&current).expect("reading");
    let (form, mut r) = typed_add(
        &current,
        &reading,
        &s,
        ("8.5", "44.25", "1.5", Some("3.25")),
    );
    let draft = draft_state(&form);
    let fresh = r.clone();
    r.expected = opened.version;
    assert!(
        refuse(&mut s, &r).contains("document changed after this form was opened"),
        "stale form"
    );
    r = fresh.clone();
    r.cut = CircularCut {
        center_mm: [66.5, 10.75],
        radius_mm: 1.875,
        extent: CutExtent::ThroughAll,
    };
    let repeated = refuse(&mut s, &r);
    assert!(
        repeated.contains(&added[1].to_string()),
        "the same disk again: {repeated}"
    );
    r.cut.center_mm = [70., 42.];
    assert!(
        refuse(&mut s, &r).contains("stay inside"),
        "outline, not box"
    );
    let saved_bytes = std::fs::read(&source).expect("saved");
    // 0 cancel after the answer, 1 stale answer, 2 failed scene, 3 failed GPU,
    // 4 cancel before the answer.
    for mode in 0..5 {
        let state = (
            s.export_path(),
            s.dirty(),
            s.can_undo(),
            s.can_redo(),
            s.session_saved_version(),
            private_files(&s),
        );
        let (g, rx) = start(&mut s, &fresh);
        if mode == 4 {
            assert!(s.cancel());
        }
        let answer = rx.recv().expect("result");
        match mode {
            0 => {
                s.cancel();
                assert_eq!(s.finish_apply(g, answer), Edited::Failed);
            }
            1 => {
                assert_eq!(s.finish_apply(g + 1, answer), Edited::Ignore);
                s.finish_apply(g, Err(CadError::Cancelled));
            }
            4 => {
                assert_eq!(s.finish_apply(g, answer), Edited::Failed);
                assert!(s.status.contains("Cancelled"), "{}", s.status);
            }
            _ => {
                assert!(matches!(s.finish_apply(g, answer), Edited::Show(_)));
                s.finish_scene(
                    g,
                    Err(CadError::kernel(if mode == 2 {
                        "scene preparation failed"
                    } else {
                        "GPU preparation failed"
                    })),
                );
            }
        }
        assert_eq!(
            (
                s.export_path(),
                s.dirty(),
                s.can_undo(),
                s.can_redo(),
                s.session_saved_version(),
                private_files(&s)
            ),
            state,
            "mode {mode}"
        );
        assert_eq!(
            draft_state(&form),
            draft,
            "the typed draft survives mode {mode}"
        );
        assert_eq!(std::fs::read(&source).expect("source"), saved_bytes);
    }
    // A third Add after Undo is accepted: Redo goes; Save As keeps the old file.
    assert!(matches!(add(&mut s, &fresh), Edited::Show(_)));
    assert!(!s.can_redo());
    let branch = root.path().join("branch.fcad");
    saved(&mut s, SaveTarget::As(branch.clone()));
    assert_eq!(std::fs::read(&source).expect("source"), saved_bytes);
    cold(&branch);
    assert_eq!(cut_ids(&branch).len(), 6);
}

#[test]
fn native_dirty_base_two_adds_edit_history_save_branch_match_cli() {
    gate("add-cut-history");
}

/// At 16 Cuts the 17th is the domain's refusal; nothing moves.
#[test]
fn native_seventeenth_cut_is_refused_and_keeps_history() {
    if !native() {
        return;
    }
    let (_root, source, reading) = fixture(16);
    assert!(
        reading.cut_bodies[0].refusal.is_some(),
        "the catalogue offers no Add"
    );
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let r = CircularCutRequest {
        source: source.clone(),
        expected: reading.version,
        body: reading.cut_bodies[0].body,
        cut: CircularCut {
            center_mm: [66.5, 10.75],
            radius_mm: 1.875,
            extent: CutExtent::ThroughAll,
        },
        destination: PathBuf::new(),
    };
    let refusal = refuse(&mut s, &r);
    assert!(refusal.contains("16"), "{refusal}");
}

/// A constrained base is not a Cut base, also after constraints were applied
/// through the session without Save; the support is not widened.
#[test]
fn native_constrained_base_refuses_add_even_after_an_unsaved_constraint_apply() {
    if !native() {
        return;
    }
    if !ferritecad_sketch_solver::is_available() {
        assert_ne!(
            std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
            Ok("1")
        );
        eprintln!("skipped: constraints need PlaneGCS");
        return;
    }
    let (_root, source, opened) = fixture(0);
    assert!(
        opened.cut_bodies[0].refusal.is_none(),
        "an unconstrained base can be cut"
    );
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let base = opened
        .sketches
        .iter()
        .find(|c| c.vertices.is_some())
        .expect("base");
    apply_constraints_native(
        &mut s,
        &ferritecad_jobs::EditSketchConstraintsRequest {
            source: source.clone(),
            expected: opened.version,
            sketch: base.sketch,
            edits: ferritecad_document::SketchConstraintEdits {
                remove: vec![],
                add: vec![ferritecad_document::AddSketchConstraint::Line(
                    ferritecad_document::AddLineConstraint::Line {
                        curve: base.vertices.as_ref().expect("vertices")[0].curve_id,
                        kind: ferritecad_document::LineConstraintKind::Horizontal,
                    },
                )],
            },
            destination: PathBuf::new(),
        },
    );
    assert!(s.dirty());
    let current = s.export_path().expect("constrained");
    let reading = read_extrude_source(&current).expect("reading");
    assert!(
        reading.cut_bodies[0].refusal.is_some(),
        "the catalogue still refuses"
    );
    assert!(
        !crate::creates::Creates::default().sketch.cuts.begin(
            &current,
            &reading,
            reading.cut_bodies[0].body
        ),
        "the Add form opened on a constrained base"
    );
    let r = CircularCutRequest {
        source: current,
        expected: reading.version,
        body: reading.cut_bodies[0].body,
        cut: CircularCut {
            center_mm: [19.25, 30.5],
            radius_mm: 2.125,
            extent: CutExtent::Blind { depth_mm: 4.875 },
        },
        destination: PathBuf::new(),
    };
    refuse(&mut s, &r);
}

#[test]
fn stub_add_cut_refuses_without_publication() {
    if ferritecad_occt::is_available() {
        eprintln!("skipped: requires no OCCT");
        return;
    }
    let (_root, source, reading) = fixture(3);
    let bytes = std::fs::read(&source).expect("source");
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let (_form, r) = typed_add(
        &source,
        &reading,
        &s,
        ("19.25", "30.5", "2.125", Some("4.875")),
    );
    let (g, rx) = start(&mut s, &r);
    let error = rx.recv().expect("answer").expect_err("refusal");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Unsupported);
    assert_eq!(s.finish_apply(g, Err(error)), Edited::Failed);
    assert!(!s.dirty() && !s.can_undo() && !s.can_redo());
    assert_eq!(private_files(&s).len(), 1);
    assert_eq!(std::fs::read(source).expect("source"), bytes);
}

#[test]
fn mixed_add_cut_uses_occt_without_solver() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        eprintln!("skipped: requires OCCT without solver");
        return;
    }
    gate("mixed-add-cut");
}
