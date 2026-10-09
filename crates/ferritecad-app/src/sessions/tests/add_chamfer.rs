// SPDX-License-Identifier: MIT
//! §30K: a new Chamfer added through the session's real worker, compared with the
//! shipped CLI. Each side mints its own UUIDs for the new Chamfer and its
//! references; only a proved bijection of those genuinely new identities is applied
//! (the one new Chamfer, then each new reference by every other cell), never to an
//! old UUID, a whole payload or an order. Undo/Redo is compared with no remap.
use super::add_cut::{Id, ids, pair_refs, same_model};
use super::chamfer::{apply as apply_distance, cold, mesh, peer as distance_peer};
use super::cut::refs;
use super::fillet::{saved, sql, state};
use super::*;
use crate::chamfers::tests::add_session::{add_draft_state, plate, typed_add};
use crate::chamfers::tests::session_apply::{saved_of, typed_distance, visible_outcome};
use ferritecad_document::{EdgeChamfer, ObjectPayload};
use ferritecad_jobs::EdgeChamferRequest;
use std::collections::BTreeMap;

fn start(
    s: &mut Sessions,
    r: &EdgeChamferRequest,
) -> (Address, mpsc::Receiver<Result<ProducedStep>>) {
    let (tx, rx) = mpsc::channel();
    let g = s
        .begin_apply(|t, _, c| {
            spawn_add_chamfer(t, r.body, r.chamfer, r.expected, c.clone(), move |v| {
                tx.send(v).expect("deliver")
            })
        })
        .expect("start");
    (g, rx)
}
/// The window's own Add: the worker, the picture, the version becoming current.
fn add(s: &mut Sessions, r: &EdgeChamferRequest) -> Edited {
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
fn refuse(s: &mut Sessions, r: &EdgeChamferRequest) -> String {
    let before = state(s);
    assert_eq!(add(s, r), Edited::Failed, "{}", s.status);
    assert_eq!(state(s), before);
    s.status.clone()
}
/// `chamfer-edge-copy` on `source`, naming the Body and the edge by UUID; the
/// joint is written in the other order, as the CLI accepts.
fn peer_add(source: &Path, body: ObjectId, chamfer: &EdgeChamfer, out: &Path) {
    let file = out.with_extension("json");
    let [a, b] = chamfer.edge.joint.segments();
    std::fs::write(
        &file,
        format!(
            r#"{{"request_version":1,"edge":{{"feature_id":"{}","joint":["{b}","{a}"]}},"distance_mm":{}}}"#,
            chamfer.edge.feature, chamfer.distance_mm
        ),
    )
    .expect("request");
    cli(&[
        "chamfer-edge-copy".as_ref(),
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
/// The one Chamfer new in `after` (since `before`), if any.
fn new_chamfer(before: &Path, after: &Path) -> Option<ObjectId> {
    let old: Vec<_> = ids(before).into_iter().map(|o| o.0).collect();
    let new: Vec<_> = ids(after)
        .into_iter()
        .filter(|o| !old.contains(&o.0))
        .collect();
    match new.as_slice() {
        [] => None,
        [(id, ObjectPayload::Chamfer(_))] => Some(*id),
        other => panic!("an Add makes exactly one Chamfer object: {other:?}"),
    }
}
/// Pairs the one new Chamfer of `ours` (after `before`) with that of `theirs`
/// (after `theirs_before`), then every new reference by all its other cells.
fn pair_chamfer(
    map: &mut BTreeMap<Id, Id>,
    before: &Path,
    ours: &Path,
    theirs_before: &Path,
    theirs: &Path,
) {
    let earlier: Vec<Id> = map.keys().copied().collect();
    let a = new_chamfer(before, ours).expect("our new Chamfer");
    let b = new_chamfer(theirs_before, theirs).expect("their new Chamfer");
    assert!(map.insert(a.to_bytes(), b.to_bytes()).is_none());
    pair_refs(map, &earlier, before, ours, theirs_before, theirs);
}
/// Old identities and cells survive an Add: every old object row is unchanged
/// except the Body (its tip); every old reference is unchanged; the new Chamfer
/// follows the base Extrude and cuts exactly the requested edge.
fn assert_allowlist(before: &Path, after: &Path, body: ObjectId, r: &EdgeChamfer) -> ObjectId {
    let old: BTreeMap<_, _> = ids(before).into_iter().collect();
    let new: BTreeMap<_, _> = ids(after).into_iter().collect();
    let added = new_chamfer(before, after).expect("one new Chamfer");
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
                "the Body tip is the new Chamfer"
            );
        } else {
            assert_eq!(payload, now, "old object {id} changed");
        }
    }
    let ObjectPayload::Chamfer(c) = &new[&added] else {
        panic!("the new feature is a Chamfer")
    };
    let ObjectPayload::Body(was) = &old[&body] else {
        panic!("Body")
    };
    assert_eq!(
        Some(c.previous),
        was.tip_feature,
        "its predecessor is the old tip"
    );
    assert_eq!(c.previous, r.edge.feature, "the base Extrude");
    assert_eq!(c.edge, r.edge, "the requested edge");
    assert_eq!(c.distance_mm, r.distance_mm);
    for x in refs(before) {
        assert!(refs(after).contains(&x), "old ref {} changed", x.id);
    }
    added
}
fn artifacts(name: &str, stl: &[u8], fbx: &[u8], exact: f64, approx: f64) {
    if let Some(dir) = std::env::var_os("FCAD_ADD_CHAMFER_SESSION_ARTIFACTS") {
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
/// Height 9.25 and left wall X −4.5 → −5.75 in the session, each equal to its CLI
/// peer in every SQL cell. Returns the CLI peer of the dirty base; each accepted
/// snapshot is appended to `accepted`.
fn dirty_base(
    s: &mut Sessions,
    source: &Path,
    root: &Path,
    accepted: &mut Vec<PathBuf>,
) -> PathBuf {
    let opened = read_extrude_source(source).expect("reading");
    let target = opened.chamfer_bodies[0].target.clone().expect("target");
    let ph = root.join("peer-height.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        source.as_os_str(),
        "--feature".as_ref(),
        target.base_feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        opened.version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        "9.25".as_ref(),
        "-o".as_ref(),
        ph.as_os_str(),
    ]);
    apply_native(s, target.base_feature, 9.25);
    same_model(
        &s.export_path().expect("height"),
        &ph,
        &BTreeMap::new(),
        "height",
    );
    let current = s.export_path().expect("current");
    accepted.push(current.clone());
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
    let pv = root.join("peer-vertices.fcad");
    sketch_peer(&ph, &v, root, &pv);
    apply_sketch_native(s, v);
    same_model(
        &s.export_path().expect("vertices"),
        &pv,
        &BTreeMap::new(),
        "vertices",
    );
    pv
}

/// The dirty plate: x from −5.75 to 33, y from 3.25 to 15.5, 9.25 mm tall.
const RECT: [f64; 4] = [-5.75, 3.25, 33., 15.5];
const A: ([f64; 2], &str) = ([-5.75, 15.5], "2.375");
const A_DISTANCE: &str = "3.0625";
const B: ([f64; 2], &str) = ([33., 3.25], "1.5");

fn gate(name: &str) {
    if !native() {
        return;
    }
    let (root, source, opened) = plate();
    let original = std::fs::read(&source).expect("source");
    let body = opened.chamfer_bodies[0].body;
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let mut accepted = vec![s.export_path().expect("opened")];
    let peer_base = dirty_base(&mut s, &source, root.path(), &mut accepted);
    let before_add = s.export_path().expect("dirty base");
    accepted.push(before_add.clone());

    // One Add through the form's own widgets, without confirming the draft.
    let reading = read_extrude_source(&before_add).expect("reading");
    let (mut form, r) = typed_add(&before_add, &reading, &s, A.0, A.1);
    assert_eq!(r.body, body, "the Body is its saved UUID");
    assert_eq!(r.expected, reading.version, "the accepted version");
    assert_eq!(
        r.source, before_add,
        "the accepted snapshot, not the user's file"
    );
    let independent = root.path().join("peer-add.fcad");
    peer_add(&peer_base, body, &r.chamfer, &independent);
    let exact = root.path().join("exact-add.fcad");
    peer_add(&before_add, body, &r.chamfer, &exact);
    assert!(matches!(add(&mut s, &r), Edited::Show(_)), "{}", s.status);
    form.finish_session_change();
    assert!(!form.active(), "the form outlived the picture it described");
    let added = s.export_path().expect("added");
    assert!(s.dirty());
    assert_eq!(
        std::fs::read(&source).expect("source"),
        original,
        "Add wrote the file"
    );
    let feature = assert_allowlist(&before_add, &added, body, &r.chamfer);
    let after = read_extrude_source(&added).expect("reading");
    assert_eq!(
        saved_of(&after).feature,
        feature,
        "Edit Chamfer distance offers it"
    );
    assert_eq!(saved_of(&after).corner.corner_mm, A.0);
    let mut exact_map = BTreeMap::new();
    pair_chamfer(&mut exact_map, &before_add, &added, &before_add, &exact);
    same_model(
        &added,
        &exact,
        &exact_map,
        "Add against the CLI on the same snapshot",
    );
    let mut map = BTreeMap::new();
    pair_chamfer(&mut map, &before_add, &added, &peer_base, &independent);
    same_model(
        &added,
        &independent,
        &map,
        "Add against the independent CLI chain",
    );
    accepted.push(added.clone());

    // A second Chamfer is the domain's refusal; nothing moves.
    assert!(
        after.chamfer_bodies[0].target.is_none(),
        "a second Chamfer is offered"
    );
    let second = refuse(
        &mut s,
        &EdgeChamferRequest {
            source: added.clone(),
            expected: after.version,
            body,
            chamfer: EdgeChamfer {
                edge: ferritecad_document::SweptEdge {
                    feature: r.chamfer.edge.feature,
                    joint: reading.chamfer_bodies[0]
                        .target
                        .as_ref()
                        .expect("target")
                        .corners
                        .iter()
                        .find(|c| c.corner_mm == B.0)
                        .expect("corner")
                        .joint,
                },
                distance_mm: 1.,
            },
            destination: PathBuf::new(),
        },
    );
    assert!(
        second.contains("and a second Chamfer are not supported yet"),
        "{second}"
    );

    // The new Chamfer's distance is changed at once through §30H's own form.
    let (mut form, d) = typed_distance(&added, &after, &s, feature, A_DISTANCE);
    let mut theirs = d.clone();
    theirs.feature = ObjectId::from_bytes(map[&feature.to_bytes()]).expect("id");
    let edited_peer = root.path().join("peer-distance.fcad");
    distance_peer(&independent, &theirs, &edited_peer);
    assert!(
        matches!(apply_distance(&mut s, &d), Edited::Show(_)),
        "{}",
        s.status
    );
    form.finish_session_change();
    let edited = s.export_path().expect("edited");
    same_model(&edited, &edited_peer, &map, "the new Chamfer's distance");
    accepted.push(edited.clone());

    // Unsaved exports are the accepted model, measured independently.
    let (stl, fbx) = export_bytes(&edited, &source, root.path(), "unsaved");
    assert_eq!(
        (stl.clone(), fbx.clone()),
        peer_bytes(&edited_peer, root.path(), "peer-unsaved"),
        "unsaved STL/FBX against the CLI"
    );
    let exact_volume = cold(&edited, RECT, A.0, 9.25);
    let approx = mesh(&stl, RECT, A.0, 3.0625, 9.25);

    // Undo/Redo within one session: no remap at all, and discovery follows.
    for expected in accepted.iter().rev().skip(1) {
        move_native(&mut s, true);
        assert_eq!(sql(&s.export_path().expect("Undo")), sql(expected));
    }
    assert!(!s.dirty());
    assert!(
        read_extrude_source(&s.export_path().expect("opened"))
            .expect("reading")
            .chamfer_features
            .is_empty(),
        "the undone Chamfer left discovery"
    );
    for expected in accepted.iter().skip(1) {
        move_native(&mut s, false);
        assert_eq!(
            sql(&s.export_path().expect("Redo")),
            sql(expected),
            "Redo returns the very same UUIDs"
        );
    }
    assert_eq!(std::fs::read(&source).expect("source"), original);

    // Save, cold rebuild, reopen; the pairing from the saved file alone is the same.
    saved(&mut s, SaveTarget::InPlace);
    same_model(&source, &edited_peer, &map, "Save");
    let mut whole = BTreeMap::new();
    pair_chamfer(&mut whole, &before_add, &source, &peer_base, &edited_peer);
    assert_eq!(
        whole, map,
        "pairing from the saved file is the step-by-step one"
    );
    cold(&source, RECT, A.0, 9.25);
    let reopened = DocumentSession::open(&source).expect("reopen");
    assert!(!reopened.is_dirty());
    drop(reopened);
    artifacts(name, &stl, &fbx, exact_volume, approx);
    let saved_bytes = std::fs::read(&source).expect("saved");

    // Undo the distance and the Add: no Chamfer again, the dirty base kept.
    move_native(&mut s, true);
    move_native(&mut s, true);
    assert!(s.can_redo());
    let current = s.export_path().expect("pre-Add");
    assert_eq!(sql(&current), sql(&before_add));
    let reading = read_extrude_source(&current).expect("reading");
    let (form, fresh) = typed_add(&current, &reading, &s, B.0, B.1);
    let draft = add_draft_state(&form);
    let mut r = fresh.clone();
    r.expected = opened.version;
    assert!(
        refuse(&mut s, &r).contains("document changed after this form was opened"),
        "stale form"
    );
    r = fresh.clone();
    // Past the shorter side (12.25) less 0.01 mm: refused, never clamped.
    r.chamfer.distance_mm = 12.2401;
    assert!(refuse(&mut s, &r).contains("too large"), "bound");
    r.chamfer.distance_mm = 0.0005;
    assert!(refuse(&mut s, &r).contains("at least 0.001 mm"), "minimum");
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

    // Another corner after Undo is accepted: Redo goes; Save As keeps the old file.
    assert!(matches!(add(&mut s, &fresh), Edited::Show(_)));
    assert!(!s.can_redo());
    let branch = root.path().join("branch.fcad");
    saved(&mut s, SaveTarget::As(branch.clone()));
    assert_eq!(std::fs::read(&source).expect("source"), saved_bytes);
    let peer_branch = root.path().join("peer-branch.fcad");
    peer_add(&peer_base, body, &fresh.chamfer, &peer_branch);
    let mut branch_map = BTreeMap::new();
    pair_chamfer(
        &mut branch_map,
        &before_add,
        &branch,
        &peer_base,
        &peer_branch,
    );
    same_model(&branch, &peer_branch, &branch_map, "the branch saved as");
    let (branch_stl, branch_fbx) = export_bytes(&branch, &branch, root.path(), "branch");
    assert_eq!(
        (branch_stl.clone(), branch_fbx.clone()),
        peer_bytes(&peer_branch, root.path(), "peer-branch"),
        "branch STL/FBX against the CLI"
    );
    let exact_volume = cold(&branch, RECT, B.0, 9.25);
    let approx = mesh(&branch_stl, RECT, B.0, 1.5, 9.25);
    artifacts(
        &format!("{name}-branch"),
        &branch_stl,
        &branch_fbx,
        exact_volume,
        approx,
    );
}

#[test]
fn native_dirty_base_add_distance_history_save_bound_and_branch_match_cli() {
    gate("add-chamfer-history");
}

/// Each of the four corners on its own fresh plate, with its own distance: the
/// Add equals the CLI on the same snapshot, and the B-Rep's named Chamfer face is
/// that corner's plane with that distance along both sides.
#[test]
fn native_every_corner_adds_its_own_plane_like_the_cli() {
    if !native() {
        return;
    }
    let rect = [-4.5, 3.25, 33., 15.5];
    for (at, distance) in [
        ([33., 15.5], "0.75"),
        ([33., 3.25], "1.5"),
        ([-4.5, 3.25], "2.25"),
        ([-4.5, 15.5], "3.0625"),
    ] {
        let (root, source, reading) = plate();
        let mut s = Sessions::default();
        s.adopt(DocumentSession::open(&source).expect("session"));
        let (_form, r) = typed_add(&source, &reading, &s, at, distance);
        let exact = root.path().join("exact.fcad");
        peer_add(&source, reading.chamfer_bodies[0].body, &r.chamfer, &exact);
        let before = s.export_path().expect("opened");
        assert!(matches!(add(&mut s, &r), Edited::Show(_)), "{}", s.status);
        let added = s.export_path().expect("added");
        assert_allowlist(&before, &added, reading.chamfer_bodies[0].body, &r.chamfer);
        let mut map = BTreeMap::new();
        pair_chamfer(&mut map, &before, &added, &source, &exact);
        same_model(&added, &exact, &map, "corner Add against the CLI");
        cold(&added, rect, at, 6.75);
    }
}

/// Vertical/Horizontal on all four Lines, the first Line's start fixed at
/// (33, 15.5), its length 14.25 mm and the second's 41 mm: §30H's set.
fn solved_rectangle(path: &Path) -> ferritecad_jobs::EditSketchConstraintsRequest {
    use ferritecad_document::{
        AddLineConstraint, AddSketchConstraint, LineConstraintKind, LineEndpoint, LineLengthMm,
        SketchConstraintEdits, SketchCoordinateMm,
    };
    let reading = read_extrude_source(path).expect("reading");
    let profile = reading
        .chamfer_bodies
        .iter()
        .find_map(|c| c.target.as_ref().map(|t| t.profile))
        .or_else(|| {
            reading
                .chamfer_features
                .first()
                .and_then(|c| c.saved.as_ref().map(|s| s.profile))
        })
        .expect("plate profile");
    let d = Document::open_read_only(path).expect("document");
    let ObjectPayload::Sketch(sketch) = d.object(profile).expect("object").expect("sketch").payload
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
        LineConstraintKind::Distance(LineLengthMm::new(14.25).expect("depth")),
    ));
    add.push(line(
        lines[1],
        LineConstraintKind::Distance(LineLengthMm::new(41.).expect("width")),
    ));
    ferritecad_jobs::EditSketchConstraintsRequest {
        source: path.to_path_buf(),
        expected: reading.version,
        sketch: profile,
        edits: SketchConstraintEdits {
            remove: Vec::new(),
            add,
        },
        destination: PathBuf::new(),
    }
}

/// An unsaved constraint Apply closes Add Chamfer for its own domain reason (a
/// managed family under a saved Chamfer is not a creation target), keeping state
/// and draft. After an accepted Add, the same constraint Apply and Apply distance
/// on the solved plate still work exactly like the CLI on the same snapshot.
#[test]
fn native_constraints_close_add_but_constraints_and_distance_after_add_still_apply() {
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
    // Constraints first: no Add on the constrained plate.
    let (_root, source, reading) = plate();
    let original = std::fs::read(&source).expect("source");
    let body = reading.chamfer_bodies[0].body;
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let (mut form, r) = typed_add(&source, &reading, &s, [-4.5, 3.25], "2.375");
    let draft = add_draft_state(&form);
    apply_constraints_native(&mut s, &solved_rectangle(&source));
    let current = s.export_path().expect("constraints");
    let constrained = read_extrude_source(&current).expect("reading");
    let reason = constrained.chamfer_bodies[0]
        .refusal
        .clone()
        .expect("a constrained plate is not a creation target");
    assert!(reason.contains("free or closure-only plate"), "{reason}");
    assert!(!crate::creates::Creates::default().sketch.chamfers.begin(
        &current,
        &constrained,
        body
    ));
    let mut fresh = r;
    fresh.expected = constrained.version;
    let refused = refuse(&mut s, &fresh);
    assert!(refused.contains("free or closure-only plate"), "{refused}");
    visible_outcome(&mut form, &s.status);
    assert_eq!(add_draft_state(&form), draft, "the draft is kept");
    assert_eq!(std::fs::read(&source).expect("source"), original);

    // Add first, then the same constraints, then a distance on the solved plate.
    let (root, source, reading) = plate();
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let (_form, r) = typed_add(&source, &reading, &s, [-4.5, 3.25], "2.375");
    assert!(matches!(add(&mut s, &r), Edited::Show(_)), "{}", s.status);
    let added = s.export_path().expect("added");
    apply_constraints_native(&mut s, &solved_rectangle(&added));
    let current = s.export_path().expect("constraints");
    let reading = read_extrude_source(&current).expect("reading");
    let stored = saved_of(&reading);
    assert!(
        stored.constrained,
        "the managed family under the saved Chamfer"
    );
    // Above the stored bound (12.24), within the solved one (14.24).
    let (_form, d) = typed_distance(&current, &reading, &s, stored.feature, "13.5");
    let out = root.path().join("distance-peer.fcad");
    distance_peer(&current, &d, &out);
    assert!(
        matches!(apply_distance(&mut s, &d), Edited::Show(_)),
        "{}",
        s.status
    );
    assert_eq!(sql(&s.export_path().expect("distance")), sql(&out));
    cold(
        &s.export_path().expect("distance"),
        [-8., 1.25, 33., 15.5],
        [-8., 1.25],
        6.75,
    );
}

#[test]
fn stub_add_chamfer_refuses_without_publication() {
    if ferritecad_occt::is_available() {
        eprintln!("skipped: requires stub");
        return;
    }
    let (_root, source, reading) = plate();
    let original = std::fs::read(&source).expect("source");
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let (_form, r) = typed_add(&source, &reading, &s, [33., 3.25], "1.5");
    let before = state(&s);
    let (g, rx) = start(&mut s, &r);
    let e = rx.recv().expect("answer").expect_err("no kernel");
    assert_eq!(e.kind(), ferritecad_types::ErrorKind::Unsupported);
    assert_eq!(s.finish_apply(g, Err(e)), Edited::Failed);
    assert_eq!(state(&s), before);
    assert_eq!(std::fs::read(source).expect("source"), original);
}

/// OCCT without PlaneGCS: the free plate takes the whole native gate.
#[test]
fn mixed_add_chamfer_uses_occt_without_solver() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        eprintln!("skipped: requires OCCT/no solver");
        return;
    }
    gate("add-chamfer-mixed");
}

/// The window scenario's inputs: corners by their stored position and distances.
const WINDOW_A: ([f64; 2], &str) = ([-5.75, 15.5], "2.375");
const WINDOW_A_DISTANCE: &str = "3.0625";
const WINDOW_BOUND: &str = "12.2401";
const WINDOW_B: ([f64; 2], &str) = ([33., 3.25], "1.5");
const WINDOW_OUTPUTS: [&str; 9] = [
    "after-confirm.fcad",
    "after-add.fcad",
    "unsaved.stl",
    "unsaved.fbx",
    "undo.stl",
    "saved.fcad",
    "after-refusal.fcad",
    "branch.fcad",
    "after-saveas.fcad",
];

fn chamfer_of(at: [f64; 2], distance: &str, path: &Path) -> EdgeChamfer {
    let reading = read_extrude_source(path).expect("reading");
    let target = reading.chamfer_bodies[0].target.clone().expect("target");
    let corner = target
        .corners
        .iter()
        .find(|c| c.corner_mm == at)
        .expect("free corner");
    EdgeChamfer {
        edge: ferritecad_document::SweptEdge {
            feature: target.base_feature,
            joint: corner.joint,
        },
        distance_mm: distance.parse().expect("distance"),
    }
}

/// §30K: actual window files against a temporary CLI chain from the untouched
/// source: height 9.25, left wall −5.75, Add A, its distance, the Undo state, and a
/// branch with Add B instead of A. New UUIDs only by `pair_chamfer`. A missing
/// output is refused before any peer job.
fn compare_gui(root: &Path) {
    for name in WINDOW_OUTPUTS {
        assert!(root.join(name).is_file(), "missing real GUI output: {name}");
    }
    let source = root.join("source/plate.fcad");
    let original = std::fs::read(&source).expect("source");
    let saved_bytes = std::fs::read(root.join("saved.fcad")).expect("saved");
    assert_eq!(
        std::fs::read(root.join("after-confirm.fcad")).expect("Confirm source"),
        original,
        "Confirm or the copy dialog wrote the user's file"
    );
    assert_eq!(
        std::fs::read(root.join("after-add.fcad")).expect("Add source"),
        original,
        "Add chamfer wrote the user's file"
    );
    assert_eq!(
        std::fs::read(root.join("after-refusal.fcad")).expect("refusal source"),
        saved_bytes,
        "a refusal wrote the user's file"
    );
    assert_eq!(
        std::fs::read(root.join("after-saveas.fcad")).expect("previous file"),
        saved_bytes,
        "Save As wrote the previous file"
    );
    let tmp = tempfile::tempdir().expect("peer root");
    let reading = read_extrude_source(&source).expect("reading");
    let target = reading.chamfer_bodies[0].target.clone().expect("target");
    let body = target.body;
    let height = tmp.path().join("height.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        source.as_os_str(),
        "--feature".as_ref(),
        target.base_feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        reading.version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        "9.25".as_ref(),
        "-o".as_ref(),
        height.as_os_str(),
    ]);
    let reading = read_extrude_source(&height).expect("height reading");
    let mut vertices = reading
        .sketches
        .iter()
        .find(|c| c.sketch == target.profile)
        .expect("base")
        .vertices
        .clone()
        .expect("vertices");
    for p in &mut vertices {
        if p.start_mm[0] == -4.5 {
            p.start_mm[0] = -5.75;
        }
    }
    let v = ferritecad_jobs::EditSketchRequest {
        source: height.clone(),
        expected: reading.version,
        sketch: target.profile,
        vertices,
        destination: PathBuf::new(),
    };
    let base = tmp.path().join("base.fcad");
    sketch_peer(&height, &v, tmp.path(), &base);
    let a = tmp.path().join("a.fcad");
    peer_add(&base, body, &chamfer_of(WINDOW_A.0, WINDOW_A.1, &base), &a);
    let last = tmp.path().join("last.fcad");
    distance_peer(
        &a,
        &ferritecad_jobs::EditChamferDistanceRequest {
            source: a.clone(),
            expected: read_extrude_source(&a).expect("a").version,
            feature: new_chamfer(&base, &a).expect("their Chamfer"),
            distance_mm: WINDOW_A_DISTANCE.parse().expect("distance"),
            destination: PathBuf::new(),
        },
        &last,
    );
    let peer_branch = tmp.path().join("branch.fcad");
    peer_add(
        &base,
        body,
        &chamfer_of(WINDOW_B.0, WINDOW_B.1, &base),
        &peer_branch,
    );

    let saved_file = root.join("saved.fcad");
    let mut map = BTreeMap::new();
    pair_chamfer(&mut map, &source, &saved_file, &base, &last);
    same_model(&saved_file, &last, &map, "saved window file");
    let branch = root.join("branch.fcad");
    let mut branch_map = BTreeMap::new();
    pair_chamfer(&mut branch_map, &source, &branch, &base, &peer_branch);
    same_model(&branch, &peer_branch, &branch_map, "window branch");
    let actual = (
        std::fs::read(root.join("unsaved.stl")).expect("stl"),
        std::fs::read(root.join("unsaved.fbx")).expect("fbx"),
    );
    assert_eq!(
        actual,
        peer_bytes(&last, tmp.path(), "last-export"),
        "unsaved STL/FBX against the CLI"
    );
    assert_eq!(
        std::fs::read(root.join("undo.stl")).expect("Undo"),
        peer_bytes(&a, tmp.path(), "undo-export").0,
        "Undo STL against the CLI"
    );
    cold(&saved_file, RECT, WINDOW_A.0, 9.25);
    cold(&branch, RECT, WINDOW_B.0, 9.25);
    mesh(&actual.0, RECT, WINDOW_A.0, 3.0625, 9.25);
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
    // Silent: inside the gated tests any output would split the harness's
    // `test … ... ok` line the CI gates read.
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
    let as_saved = |r: &Path, bytes: &[u8]| {
        for name in ["saved.fcad", "after-refusal.fcad", "after-saveas.fcad"] {
            std::fs::write(r.join(name), bytes).expect("write");
        }
    };
    control(root, "original instead of Save", "our new Chamfer", &|r| {
        as_saved(r, &file(r, "source/plate.fcad"))
    });
    control(root, "branch instead of Save", "a new ref pairs", &|r| {
        as_saved(r, &file(r, "branch.fcad"))
    });
    control(root, "Save instead of branch", "a new ref pairs", &|r| {
        std::fs::write(r.join("branch.fcad"), file(r, "saved.fcad")).expect("write")
    });
    control(
        root,
        "stale export",
        "unsaved STL/FBX against the CLI",
        &|r| std::fs::write(r.join("unsaved.stl"), file(r, "undo.stl")).expect("write"),
    );
    control(
        root,
        "Confirm wrote the file",
        "Confirm or the copy dialog wrote the user's file",
        &|r| std::fs::write(r.join("after-confirm.fcad"), file(r, "saved.fcad")).expect("write"),
    );
    // A new reference may map only its own UUID, never its owner.
    control(root, "new ref owner", "a new ref pairs", &|r| {
        let saved = r.join("saved.fcad");
        let source = r.join("source/plate.fcad");
        let base = read_extrude_source(&source)
            .expect("reading")
            .chamfer_bodies[0]
            .target
            .clone()
            .expect("target")
            .base_feature;
        let new = refs(&saved)
            .into_iter()
            .find(|x| !refs(&source).iter().any(|o| o.id == x.id))
            .expect("a new ref");
        let db = rusqlite::Connection::open(&saved).expect("db");
        db.execute(
            "UPDATE topology_refs SET owner_id=?1 WHERE id=?2",
            rusqlite::params![base.to_bytes().as_slice(), new.id.to_bytes().as_slice()],
        )
        .expect("wrong owner");
        drop(db);
        as_saved(r, &file(r, "saved.fcad"));
    });
}

/// The window scenario run by the session's own forms and workers, its files laid
/// out as the window leaves them, then the comparator and its controls. This is a
/// self-check of the comparator, not window evidence.
#[test]
fn native_window_scenario_on_session_files_passes_the_comparator_and_its_controls() {
    if !native() {
        return;
    }
    let (root, source, opened) = plate();
    let layout = root.path().join("layout");
    std::fs::create_dir_all(layout.join("source")).expect("layout");
    let put = |name: &str, bytes: &[u8]| std::fs::write(layout.join(name), bytes).expect(name);
    put(
        "source/plate.fcad",
        &std::fs::read(&source).expect("source"),
    );
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    // Confirm and the request history are the form's own; nothing is written.
    let (form, _) = typed_add(&source, &opened, &s, WINDOW_B.0, WINDOW_B.1);
    drop(form);
    assert!(!s.dirty());
    put(
        "after-confirm.fcad",
        &std::fs::read(&source).expect("after Confirm"),
    );
    let mut accepted = vec![s.export_path().expect("opened")];
    dirty_base(&mut s, &source, root.path(), &mut accepted);
    let current = s.export_path().expect("base");
    let reading = read_extrude_source(&current).expect("reading");
    let (mut form, r) = typed_add(&current, &reading, &s, WINDOW_A.0, WINDOW_A.1);
    assert!(matches!(add(&mut s, &r), Edited::Show(_)), "{}", s.status);
    form.finish_session_change();
    let added = s.export_path().expect("A");
    let reading = read_extrude_source(&added).expect("reading");
    let feature = new_chamfer(&current, &added).expect("ours");
    let (mut form, d) = typed_distance(&added, &reading, &s, feature, WINDOW_A_DISTANCE);
    assert!(matches!(apply_distance(&mut s, &d), Edited::Show(_)));
    form.finish_session_change();
    put("after-add.fcad", &std::fs::read(&source).expect("source"));
    let last = s.export_path().expect("distance");
    let (stl, fbx) = export_bytes(&last, &source, root.path(), "unsaved");
    put("unsaved.stl", &stl);
    put("unsaved.fbx", &fbx);
    move_native(&mut s, true);
    let undo = export_bytes(
        &s.export_path().expect("Undo"),
        &source,
        root.path(),
        "undo",
    )
    .0;
    put("undo.stl", &undo);
    for _ in 0..3 {
        move_native(&mut s, true);
    }
    assert!(!s.dirty());
    for _ in 0..4 {
        move_native(&mut s, false);
    }
    saved(&mut s, SaveTarget::InPlace);
    put("saved.fcad", &std::fs::read(&source).expect("saved"));
    move_native(&mut s, true);
    move_native(&mut s, true);
    let current = s.export_path().expect("pre-Add");
    let reading = read_extrude_source(&current).expect("reading");
    let (_, mut bound) = typed_add(&current, &reading, &s, WINDOW_A.0, WINDOW_A.1);
    bound.chamfer.distance_mm = WINDOW_BOUND.parse().expect("bound");
    assert!(refuse(&mut s, &bound).contains("too large"));
    put(
        "after-refusal.fcad",
        &std::fs::read(&source).expect("after refusal"),
    );
    let (mut form, r) = typed_add(&current, &reading, &s, WINDOW_B.0, WINDOW_B.1);
    assert!(matches!(add(&mut s, &r), Edited::Show(_)), "{}", s.status);
    form.finish_session_change();
    assert!(!s.can_redo());
    let branch = root.path().join("branch.fcad");
    saved(&mut s, SaveTarget::As(branch.clone()));
    put("branch.fcad", &std::fs::read(&branch).expect("branch"));
    put(
        "after-saveas.fcad",
        &std::fs::read(&source).expect("previous"),
    );
    compare_with_controls(&layout);
}

#[test]
fn native_compare_real_add_chamfer_gui_artifacts_with_negative_controls() {
    let Some(root) = std::env::var_os("FCAD_30K_GUI_DIR") else {
        eprintln!("skipped: requires real GUI artifacts");
        return;
    };
    assert!(native());
    compare_with_controls(Path::new(&root));
    println!("FCAD_30K_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true");
}
