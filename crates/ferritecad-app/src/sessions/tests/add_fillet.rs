// SPDX-License-Identifier: MIT
//! §30J: a new Fillet added through the session's real worker, compared with the
//! shipped CLI. Each side mints its own UUIDs for what is new; only a proved
//! bijection of genuinely new identities is applied (new Fillets by their place in
//! the history, then new references by every other cell), never to an old UUID, a
//! whole payload or an order. Undo/Redo is compared with no remap.
use super::add_cut::{Id, ids, pair_refs, same_model};
use super::cut::refs;
use super::fillet::{
    apply as apply_radius, cold, history, mesh, peer as radius_peer, saved, state,
};
use super::*;
use crate::fillets::tests::add_session::{add_draft_state, typed_add};
use crate::fillets::tests::session_apply::{fixture, typed_radius, visible_outcome};
use ferritecad_document::{EdgeFillet, ObjectPayload};
use ferritecad_jobs::EdgeFilletRequest;
use std::collections::BTreeMap;

fn start(s: &mut Sessions, r: &EdgeFilletRequest) -> (u64, mpsc::Receiver<Result<ProducedStep>>) {
    let (tx, rx) = mpsc::channel();
    let g = s
        .begin_apply(|t, _, c| {
            spawn_add_fillet(t, r.body, r.fillet, r.expected, c.clone(), move |v| {
                tx.send(v).expect("deliver")
            })
        })
        .expect("start");
    (g, rx)
}
/// The window's own Add: the worker, the picture, the version becoming current.
fn add(s: &mut Sessions, r: &EdgeFilletRequest) -> Edited {
    let (g, rx) = start(s, r);
    let edited = s.finish_apply(
        g,
        rx.recv_timeout(std::time::Duration::from_secs(120))
            .expect("answer"),
    );
    if let Edited::Show(path) = &edited {
        let (tx, rx) = mpsc::channel();
        let worker = spawn_scene(path.clone(), s.scene_token(g).expect("token"), move |v| {
            tx.send(v).expect("scene")
        });
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
fn refuse(s: &mut Sessions, r: &EdgeFilletRequest) -> String {
    let before = state(s);
    assert_eq!(add(s, r), Edited::Failed, "{}", s.status);
    assert_eq!(state(s), before);
    s.status.clone()
}
/// `fillet-edge-copy` on `source`, naming the Body and the edge by UUID; the
/// joint is written in the other order, as the CLI accepts.
fn peer_add(source: &Path, body: ObjectId, fillet: &EdgeFillet, out: &Path) {
    let file = out.with_extension("json");
    let [a, b] = fillet.edge.joint.segments();
    std::fs::write(
        &file,
        format!(
            r#"{{"request_version":1,"edge":{{"feature_id":"{}","joint":["{b}","{a}"]}},"radius_mm":{}}}"#,
            fillet.edge.feature, fillet.radius_mm
        ),
    )
    .expect("request");
    cli(&[
        "fillet-edge-copy".as_ref(),
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

/// The Fillets new in `after` (since `before`), in history order: each one's
/// `previous` is the one before it, the first follows an old feature.
fn history_of(before: &Path, after: &Path) -> Vec<ObjectId> {
    let old: Vec<_> = ids(before).into_iter().map(|o| o.0).collect();
    let new: BTreeMap<_, _> = ids(after)
        .into_iter()
        .filter(|o| !old.contains(&o.0))
        .collect();
    let mut order: Vec<ObjectId> = Vec::new();
    loop {
        let next: Vec<_> = new
            .iter()
            .filter(|(id, p)| match p {
                ObjectPayload::Fillet(f) if !order.contains(id) => match order.last() {
                    Some(last) => f.previous == *last,
                    None => !new.contains_key(&f.previous),
                },
                _ => false,
            })
            .collect();
        assert!(next.len() <= 1, "the new Fillets branch");
        let Some((id, _)) = next.first() else {
            return order;
        };
        order.push(**id);
    }
}

/// Pairs what is new in `ours` (after `before`) with what is new in `theirs`
/// (after `theirs_before`), extending `map`: `adds` new Fillets on each side by
/// their place in the history (never by rowid, ordinal or name), then every new
/// reference by all its other cells. Nothing old is mapped.
fn pair_fillets(
    map: &mut BTreeMap<Id, Id>,
    adds: usize,
    before: &Path,
    ours: &Path,
    theirs_before: &Path,
    theirs: &Path,
) {
    let earlier: Vec<Id> = map.keys().copied().collect();
    let count = |before: &Path, after: &Path| {
        let old: Vec<_> = ids(before).into_iter().map(|o| o.0).collect();
        ids(after).iter().filter(|o| !old.contains(&o.0)).count()
    };
    assert_eq!(
        count(before, ours),
        adds,
        "each Add makes one Fillet object"
    );
    assert_eq!(count(theirs_before, theirs), adds);
    let (a, b) = (history_of(before, ours), history_of(theirs_before, theirs));
    assert_eq!((a.len(), b.len()), (adds, adds), "one history chain");
    for (u, v) in a.into_iter().zip(b) {
        assert!(map.insert(u.to_bytes(), v.to_bytes()).is_none());
    }
    pair_refs(map, &earlier, before, ours, theirs_before, theirs);
}

/// Old identities and cells survive an Add: every old object row is unchanged
/// except the Body (its tip); every old reference is unchanged; the new Fillet
/// follows the old tip and rounds exactly the requested edge.
fn assert_allowlist(before: &Path, after: &Path, body: ObjectId, r: &EdgeFillet) -> ObjectId {
    let old: BTreeMap<_, _> = ids(before).into_iter().collect();
    let new: BTreeMap<_, _> = ids(after).into_iter().collect();
    let added = history_of(before, after);
    assert_eq!(added.len(), 1);
    let added = added[0];
    for (id, payload) in &old {
        let now = new.get(id).expect("an old object vanished");
        if *id == body {
            let (ObjectPayload::Body(was), ObjectPayload::Body(is)) = (payload, now) else {
                panic!("Body")
            };
            assert_ne!(was.tip_feature, Some(added));
            assert_eq!(
                is.tip_feature,
                Some(added),
                "the Body tip is the new Fillet"
            );
        } else {
            assert_eq!(payload, now, "old object {id} changed");
        }
    }
    let ObjectPayload::Fillet(f) = &new[&added] else {
        panic!("the new feature is a Fillet")
    };
    let ObjectPayload::Body(was) = &old[&body] else {
        panic!("Body")
    };
    assert_eq!(
        Some(f.previous),
        was.tip_feature,
        "its predecessor is the old tip"
    );
    assert_eq!(f.edge, r.edge, "the requested edge");
    assert_eq!(f.radius_mm, r.radius_mm);
    for x in refs(before) {
        assert!(refs(after).contains(&x), "old ref {} changed", x.id);
    }
    added
}

/// Corners in the stored drawing after the left wall moved to −5.75, and radii:
/// different, fractional, two pairs sharing a Line.
const ADDS: [([f64; 2], &str); 4] = [
    ([33., 3.25], "2.375"),
    ([-5.75, 15.5], "3.0625"),
    ([33., 15.5], "1.5"),
    ([-5.75, 3.25], "4.25"),
];

fn artifacts(name: &str, stl: &[u8], fbx: &[u8], exact: f64, approx: f64) {
    if let Some(dir) = std::env::var_os("FCAD_ADD_FILLET_SESSION_ARTIFACTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join(format!("{name}.stl")), stl).expect("stl");
        std::fs::write(dir.join(format!("{name}.fbx")), fbx).expect("fbx");
        std::fs::write(
            dir.join(format!("{name}.metrics.txt")),
            format!("analytical_mm3={exact:.9} stl_mm3={approx:.9}\n"),
        )
        .expect("metrics");
    }
}

fn gate(name: &str) {
    if !native() {
        return;
    }
    let (root, source, opened) = fixture(0);
    let original = std::fs::read(&source).expect("source");
    let target = opened.fillet_bodies[0].target.clone().expect("target");
    let (body, base) = (target.body, target.base_feature);
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    // Dirty base: height, then the left wall's two vertices.
    let ph = root.path().join("peer-height.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        source.as_os_str(),
        "--feature".as_ref(),
        base.to_string().as_ref(),
        "--expect-version".as_ref(),
        opened.version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        "9.25".as_ref(),
        "-o".as_ref(),
        ph.as_os_str(),
    ]);
    apply_native(&mut s, base, 9.25);
    same_model(
        &s.export_path().expect("height"),
        &ph,
        &BTreeMap::new(),
        "height",
    );
    let current = s.export_path().expect("current");
    let reading = read_extrude_source(&current).expect("reading");
    let mut v = ferritecad_jobs::EditSketchRequest {
        source: current,
        expected: reading.version,
        sketch: target.profile,
        vertices: reading
            .sketches
            .iter()
            .find(|c| c.sketch == target.profile)
            .expect("base")
            .vertices
            .clone()
            .expect("vertices"),
        destination: PathBuf::new(),
    };
    for p in &mut v.vertices {
        if p.start_mm[0] == -4.5 {
            p.start_mm[0] = -5.75;
        }
    }
    let mut peer_before = root.path().join("peer-vertices.fcad");
    sketch_peer(&ph, &v, root.path(), &peer_before);
    apply_sketch_native(&mut s, v);
    let mut map = BTreeMap::new();
    same_model(
        &s.export_path().expect("vertices"),
        &peer_before,
        &map,
        "vertices",
    );
    let before_fillets = s.export_path().expect("before Fillets");
    let mut accepted = vec![before_fillets.clone()];
    let mut added = Vec::new();

    // Four Adds through the form's own widgets, without confirming the draft.
    for (step, (at, radius)) in ADDS.into_iter().enumerate() {
        let current = s.export_path().expect("current");
        let reading = read_extrude_source(&current).expect("reading");
        let (mut form, r) = typed_add(&current, &reading, &s, at, radius);
        assert_eq!(r.body, body, "the Body is its saved UUID");
        assert_eq!(r.expected, reading.version, "the accepted version");
        assert_eq!(
            r.source, current,
            "the accepted snapshot, not the user's file"
        );
        assert_eq!(r.fillet.edge.feature, base);
        let independent = root.path().join(format!("peer-add-{step}.fcad"));
        peer_add(&peer_before, body, &r.fillet, &independent);
        let exact = root.path().join(format!("exact-add-{step}.fcad"));
        peer_add(&current, body, &r.fillet, &exact);
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
        let feature = assert_allowlist(&current, &applied, body, &r.fillet);
        let saved = history(&applied);
        assert_eq!(saved.len(), step + 1);
        assert_eq!(
            saved[step].feature, feature,
            "Edit Fillet radius offers it last"
        );
        assert_eq!(saved[step].corner.corner_mm, at);
        let mut exact_map = BTreeMap::new();
        pair_fillets(&mut exact_map, 1, &current, &applied, &current, &exact);
        same_model(
            &applied,
            &exact,
            &exact_map,
            "Add against the CLI on the same snapshot",
        );
        pair_fillets(&mut map, 1, &current, &applied, &peer_before, &independent);
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
    // A fifth: no free corner is left, so a request at a rounded one is refused.
    let current = s.export_path().expect("four");
    let full = read_extrude_source(&current).expect("reading");
    assert!(
        full.fillet_bodies[0].target.is_none() || full.fillet_bodies[0].refusal.is_some(),
        "a fifth Fillet is offered"
    );
    let first = history(&current)[0].clone();
    let fifth = EdgeFilletRequest {
        source: current.clone(),
        expected: full.version,
        body,
        fillet: EdgeFillet {
            edge: first.edge,
            radius_mm: 1.,
        },
        destination: PathBuf::new(),
    };
    let reason = refuse(&mut s, &fifth);
    assert!(
        reason.contains("every corner of this plate is already rounded"),
        "{reason}"
    );

    // The newest Fillet is edited at once through §30G's own form and worker.
    let reading = read_extrude_source(&current).expect("reading");
    let (mut form, r) = typed_radius(&current, &reading, &s, added[3], "4.5");
    let mut theirs = r.clone();
    theirs.feature = ObjectId::from_bytes(map[&r.feature.to_bytes()]).expect("id");
    let edited_peer = root.path().join("peer-edit.fcad");
    radius_peer(&peer_before, &theirs, &edited_peer);
    assert!(
        matches!(apply_radius(&mut s, &r), Edited::Show(_)),
        "{}",
        s.status
    );
    form.finish_session_change();
    let edited = s.export_path().expect("edited");
    same_model(&edited, &edited_peer, &map, "the new Fillet edited");
    accepted.push(edited.clone());
    peer_before = edited_peer;

    // Unsaved exports are the accepted model, measured independently.
    let (stl, fbx) = export_bytes(&edited, &source, root.path(), "unsaved");
    assert_eq!(
        (stl.clone(), fbx.clone()),
        peer_bytes(&peer_before, root.path(), "peer-unsaved"),
        "unsaved STL/FBX against the CLI"
    );
    let exact = cold(&edited, 38.75, 12.25, 9.25);
    let approx = mesh(&stl, &history(&edited), [-5.75, 3.25, 33., 15.5], 9.25);

    // Undo/Redo within one session: no remap at all, and discovery follows.
    for expected in accepted.iter().rev().skip(1) {
        move_native(&mut s, true);
        assert_eq!(
            super::fillet::sql(&s.export_path().expect("Undo")),
            super::fillet::sql(expected)
        );
    }
    let now = s.export_path().expect("before Fillets");
    assert!(history(&now).is_empty(), "undone Fillets left discovery");
    move_native(&mut s, true);
    move_native(&mut s, true);
    assert!(!s.dirty());
    move_native(&mut s, false);
    move_native(&mut s, false);
    for expected in accepted.iter().skip(1) {
        move_native(&mut s, false);
        assert_eq!(
            super::fillet::sql(&s.export_path().expect("Redo")),
            super::fillet::sql(expected),
            "Redo returns the very same UUIDs"
        );
    }
    assert_eq!(
        history(&s.export_path().expect("Redo"))
            .iter()
            .map(|f| f.feature)
            .collect::<Vec<_>>(),
        added
    );
    assert_eq!(std::fs::read(&source).expect("source"), original);

    // Save, cold rebuild, reopen; the four Adds paired at once from the saved file.
    saved(&mut s, SaveTarget::InPlace);
    same_model(&source, &peer_before, &map, "Save");
    let mut whole = BTreeMap::new();
    let peer_base = root.path().join("peer-vertices.fcad");
    pair_fillets(
        &mut whole,
        4,
        &before_fillets,
        &source,
        &peer_base,
        &peer_before,
    );
    assert_eq!(whole, map, "history-order pairing is the step-by-step one");
    cold(&source, 38.75, 12.25, 9.25);
    let reopened = DocumentSession::open(&source).expect("reopen");
    assert!(!reopened.is_dirty());
    drop(reopened);
    artifacts(name, &stl, &fbx, exact, approx);
    let saved_bytes = std::fs::read(&source).expect("saved");

    // Undo the edit and the fourth Add: three Fillets, one free corner.
    move_native(&mut s, true);
    move_native(&mut s, true);
    assert!(s.can_redo());
    let current = s.export_path().expect("three");
    assert_eq!(
        super::fillet::sql(&current),
        super::fillet::sql(&accepted[3])
    );
    let reading = read_extrude_source(&current).expect("reading");
    let (form, fresh) = typed_add(&current, &reading, &s, [-5.75, 3.25], "2.75");
    let draft = add_draft_state(&form);
    let mut r = fresh.clone();
    r.expected = opened.version;
    assert!(
        refuse(&mut s, &r).contains("document changed after this form was opened"),
        "stale form"
    );
    // The same corner again is the domain's refusal naming the Fillet there.
    r = fresh.clone();
    r.fillet.edge = history(&current)[0].edge;
    let repeated = refuse(&mut s, &r);
    assert!(
        repeated.contains("already rounded") && repeated.contains(&added[0].to_string()),
        "{repeated}"
    );
    r = fresh.clone();
    r.fillet.radius_mm = 6.2;
    assert!(
        refuse(&mut s, &r).contains("too large"),
        "half the shorter side"
    );
    // 0 cancel after the answer, 1 stale answer, 2 failed scene, 3 failed GPU,
    // 4 cancel during binding, 5 cancel before the answer.
    let (tx, rx) = mpsc::channel();
    let worker = spawn_scene(current.clone(), CancelToken::new(), move |v| {
        tx.send(v).expect("scene")
    });
    let loaded = rx.recv().expect("scene").expect("accepted picture");
    worker.join().expect("worker");
    let mut camera = crate::ViewportInput::new();
    let next = crate::prepare_load(&camera, &current, Ok(loaded), |snapshot, _| Ok(snapshot))
        .expect("prepared accepted scene");
    let mut live = crate::LiveScene::new(
        None,
        Arc::clone(&next.prepared),
        Vec::new(),
        crate::FaceNames::default(),
        crate::EdgeNames::default(),
        crate::VertexNames::default(),
        crate::Visibility::default(),
        Vec::new(),
    );
    crate::commit_scene(&mut live, &mut camera, Ok(next)).expect("accepted scene");
    let kept = Arc::clone(&live.prepared);
    for mode in 0..6 {
        let before = state(&s);
        let (g, rx) = start(&mut s, &fresh);
        if mode == 5 {
            assert!(s.cancel());
        }
        let answer = rx.recv().expect("answer");
        match mode {
            0 => {
                s.cancel();
                assert_eq!(s.finish_apply(g, answer), Edited::Failed);
            }
            1 => {
                assert_eq!(s.finish_apply(g + 1, answer), Edited::Ignore);
                s.finish_apply(g, Err(CadError::Cancelled));
            }
            5 => {
                assert_eq!(s.finish_apply(g, answer), Edited::Failed);
                assert!(s.status.contains("Cancelled"), "{}", s.status);
            }
            _ => {
                assert!(matches!(s.finish_apply(g, answer), Edited::Show(_)));
                let candidate = s.staged_path(g).expect("candidate");
                let loaded = if mode == 2 {
                    Err(CadError::kernel("scene preparation failed"))
                } else {
                    let (tx, rx) = mpsc::channel();
                    let worker = spawn_scene(
                        candidate.clone(),
                        s.scene_token(g).expect("token"),
                        move |v| tx.send(v).expect("scene"),
                    );
                    assert!(s.attach_scene(g, worker));
                    rx.recv().expect("scene")
                };
                let mut called = false;
                let next = crate::prepare_load(&camera, &candidate, loaded, |snapshot, _| {
                    called = true;
                    if mode == 3 {
                        Err(CadError::kernel("GPU preparation failed"))
                    } else {
                        Ok(snapshot)
                    }
                });
                assert_eq!(called, mode != 2, "actual preparation seam");
                if mode == 4 {
                    s.cancel();
                }
                let next = next.and_then(|next| s.bind(Bind::Staged).map(|()| next));
                let outcome = crate::commit_scene(&mut live, &mut camera, next);
                assert!(outcome.is_err(), "failed/cancelled picture must not commit");
                assert!(s.finish_scene(g, outcome));
                assert!(Arc::ptr_eq(&live.prepared, &kept));
                assert_eq!(live.document.as_deref(), Some(current.as_path()));
            }
        }
        assert_eq!(state(&s), before, "mode {mode}");
        assert_eq!(
            add_draft_state(&form),
            draft,
            "the draft survives mode {mode}"
        );
        assert_eq!(std::fs::read(&source).expect("source"), saved_bytes);
    }
    let mut form = form;
    visible_outcome(&mut form, &s.status);

    // An Add after Undo is accepted: Redo goes; Save As keeps the old file.
    assert!(matches!(add(&mut s, &fresh), Edited::Show(_)));
    assert!(!s.can_redo());
    let branch = root.path().join("branch.fcad");
    saved(&mut s, SaveTarget::As(branch.clone()));
    assert_eq!(std::fs::read(&source).expect("source"), saved_bytes);
    let peer_three = root.path().join("peer-add-2.fcad");
    let peer_branch = root.path().join("peer-branch.fcad");
    peer_add(&peer_three, body, &fresh.fillet, &peer_branch);
    pair_fillets(
        &mut map,
        1,
        &accepted[3],
        &branch,
        &peer_three,
        &peer_branch,
    );
    same_model(&branch, &peer_branch, &map, "the branch saved as");
    let (branch_stl, branch_fbx) = export_bytes(&branch, &branch, root.path(), "branch");
    assert_eq!(
        (branch_stl.clone(), branch_fbx.clone()),
        peer_bytes(&peer_branch, root.path(), "peer-branch"),
        "branch STL/FBX against the CLI"
    );
    let exact = cold(&branch, 38.75, 12.25, 9.25);
    let approx = mesh(
        &branch_stl,
        &history(&branch),
        [-5.75, 3.25, 33., 15.5],
        9.25,
    );
    artifacts(
        &format!("{name}-branch"),
        &branch_stl,
        &branch_fbx,
        exact,
        approx,
    );
}

#[test]
fn native_dirty_base_four_adds_radius_edit_history_save_branch_match_cli() {
    gate("add-fillet-history");
}

/// On an accepted unsaved constraint Apply the bound is the solved plate's: the
/// stored drawing (12.25 mm deep) allows r 5.5, the solved one (10 mm) does not.
#[test]
fn native_add_on_unsaved_constraints_is_measured_on_solved_lines() {
    if !native() {
        return;
    }
    if !ferritecad_sketch_solver::is_available() {
        assert_ne!(
            std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
            Ok("1")
        );
        eprintln!("skipped: requires solver");
        return;
    }
    let (root, source, reading) = fixture(0);
    let original = std::fs::read(&source).expect("source");
    let target = reading.fillet_bodies[0].target.clone().expect("target");
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    apply_constraints_native(&mut s, &solved_rectangle(&source, &reading, 10.));
    let current = s.export_path().expect("constraints");
    let reading = read_extrude_source(&current).expect("reading");
    let tgt = reading.fillet_bodies[0]
        .target
        .as_ref()
        .expect("constrained target");
    assert!(tgt.constrained, "the Add form sees a constrained plate");
    // Refused on the solved plate although the stored guess would allow it.
    let (mut form, r) = typed_add(&current, &reading, &s, [33., 15.5], "5.5");
    let corner = tgt
        .corners
        .iter()
        .find(|c| c.corner_mm == [33., 15.5])
        .expect("corner");
    assert!(corner.max_radius_mm >= 5.5, "the stored guess allows r 5.5");
    let draft = add_draft_state(&form);
    let before = state(&s);
    let (g, rx) = start(&mut s, &r);
    let error = rx.recv().expect("answer").expect_err("solved refusal");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
    assert!(error.to_string().contains("too large"), "{error}");
    assert_eq!(s.finish_apply(g, Err(error)), Edited::Failed);
    visible_outcome(&mut form, &s.status);
    assert_eq!(add_draft_state(&form), draft);
    assert_eq!(state(&s), before);
    // Accepted on the solved plate, same as the CLI on the same snapshot.
    let (mut form, r) = typed_add(&current, &reading, &s, [33., 15.5], "4.75");
    let exact = root.path().join("exact.fcad");
    peer_add(&current, target.body, &r.fillet, &exact);
    assert!(matches!(add(&mut s, &r), Edited::Show(_)), "{}", s.status);
    form.finish_session_change();
    let applied = s.export_path().expect("added");
    assert_allowlist(&current, &applied, target.body, &r.fillet);
    let mut map = BTreeMap::new();
    pair_fillets(&mut map, 1, &current, &applied, &current, &exact);
    same_model(&applied, &exact, &map, "constrained Add against the CLI");
    // Solved: x from −8 to 33, y from 5.5 to 15.5, 6.75 mm tall.
    cold(&applied, 41., 10., 6.75);
    let (stl, fbx) = export_bytes(&applied, &source, root.path(), "constrained");
    assert_eq!(
        (stl.clone(), fbx.clone()),
        peer_bytes(&exact, root.path(), "peer-constrained")
    );
    let mut solved = history(&applied);
    solved[0].corner.corner_mm = [33., 15.5];
    mesh(&stl, &solved, [-8., 5.5, 33., 15.5], 6.75);
    move_native(&mut s, true);
    move_native(&mut s, false);
    assert_eq!(
        super::fillet::sql(&s.export_path().expect("Redo")),
        super::fillet::sql(&applied)
    );
    assert_eq!(std::fs::read(&source).expect("source"), original);
}

/// Vertical/Horizontal on all four Lines, the first Line's start fixed at
/// (33, 15.5), its length `depth` and the second's 41 mm.
fn solved_rectangle(
    source: &Path,
    reading: &ferritecad_document::ExtrudeEditSource,
    depth: f64,
) -> ferritecad_jobs::EditSketchConstraintsRequest {
    use ferritecad_document::{
        AddLineConstraint, AddSketchConstraint, LineConstraintKind, LineEndpoint, LineLengthMm,
        SketchConstraintEdits, SketchCoordinateMm,
    };
    let target = reading.fillet_bodies[0].target.as_ref().expect("target");
    let d = Document::open_read_only(source).expect("document");
    let ObjectPayload::Sketch(sketch) = d
        .object(target.profile)
        .expect("object")
        .expect("sketch")
        .payload
    else {
        panic!("Sketch");
    };
    let lines: Vec<_> = sketch.curves.iter().map(|c| c.id).collect();
    let line = |curve, kind| AddSketchConstraint::Line(AddLineConstraint::Line { curve, kind });
    let mut add: Vec<_> = (0..4)
        .map(|i| {
            line(
                lines[i],
                if i % 2 == 0 {
                    LineConstraintKind::Vertical
                } else {
                    LineConstraintKind::Horizontal
                },
            )
        })
        .collect();
    add.push(line(
        lines[0],
        LineConstraintKind::Fixed {
            at: LineEndpoint::Start,
            x: SketchCoordinateMm::new(33.).expect("x"),
            y: SketchCoordinateMm::new(15.5).expect("y"),
        },
    ));
    add.push(line(
        lines[0],
        LineConstraintKind::Distance(LineLengthMm::new(depth).expect("depth")),
    ));
    add.push(line(
        lines[1],
        LineConstraintKind::Distance(LineLengthMm::new(41.).expect("width")),
    ));
    ferritecad_jobs::EditSketchConstraintsRequest {
        source: source.to_path_buf(),
        expected: reading.version,
        sketch: target.profile,
        edits: SketchConstraintEdits {
            remove: Vec::new(),
            add,
        },
        destination: PathBuf::new(),
    }
}

#[test]
fn stub_add_fillet_refuses_without_publication() {
    if ferritecad_occt::is_available() {
        eprintln!("skipped: requires stub");
        return;
    }
    let (_root, source, reading) = fixture(0);
    let original = std::fs::read(&source).expect("source");
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let (_form, r) = typed_add(&source, &reading, &s, [33., 3.25], "2.375");
    let before = state(&s);
    let (g, rx) = start(&mut s, &r);
    let e = rx.recv().expect("answer").expect_err("no kernel");
    assert_eq!(e.kind(), ferritecad_types::ErrorKind::Unsupported);
    assert_eq!(s.finish_apply(g, Err(e)), Edited::Failed);
    assert_eq!(state(&s), before);
    assert_eq!(std::fs::read(source).expect("source"), original);
}

/// OCCT without PlaneGCS: the free plate adds exactly as natively; a constrained
/// plate, written by the shipped preparation, refuses without a solver.
#[test]
fn mixed_add_fillet_uses_occt_without_solver() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        eprintln!("skipped: requires OCCT/no solver");
        return;
    }
    gate("add-fillet-mixed");
    let (_root, source, reading) = fixture(0);
    let r = solved_rectangle(&source, &reading, 10.);
    let mut d = Document::open(&source).expect("document");
    let prepared = ferritecad_document::prepare_sketch_constraints(&d, r.sketch, &r.edits)
        .expect("prepared constraints");
    d.write_sketch_constraints(&prepared).expect("written");
    d.close().expect("close");
    let reading = read_extrude_source(&source).expect("reading");
    let original = std::fs::read(&source).expect("source");
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let (_form, r) = typed_add(&source, &reading, &s, [33., 15.5], "2.375");
    let before = state(&s);
    let (g, rx) = start(&mut s, &r);
    let e = rx.recv().expect("answer").expect_err("no solver");
    assert_eq!(e.kind(), ferritecad_types::ErrorKind::Unsupported, "{e}");
    assert_eq!(s.finish_apply(g, Err(e)), Edited::Failed);
    assert_eq!(state(&s), before);
    assert_eq!(std::fs::read(source).expect("source"), original);
}
