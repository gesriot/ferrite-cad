// SPDX-License-Identifier: MIT
//! §30H: real workers, UUID selection, exact SQL and independent geometry peers.
use super::fillet::{saved, sql, state};
use super::*;
use crate::chamfers::tests::session_apply::{
    CORNER, draft_state, fixture, saved_of, typed_distance, visible_outcome,
};
use ferritecad_document::{MIN_FLAT_MM, ObjectPayload, SemanticRole};
use ferritecad_jobs::EditChamferDistanceRequest;
use ferritecad_kernel::{FaceSurface, GeometryKernel};

fn start(
    s: &mut Sessions,
    r: &EditChamferDistanceRequest,
) -> (u64, mpsc::Receiver<Result<ProducedStep>>) {
    let (tx, rx) = mpsc::channel();
    let g = s
        .begin_apply(|t, _, c| {
            spawn_apply_chamfer_distance(
                t,
                r.feature,
                r.distance_mm,
                r.expected,
                c.clone(),
                move |v| tx.send(v).expect("deliver"),
            )
        })
        .expect("start");
    (g, rx)
}
fn apply(s: &mut Sessions, r: &EditChamferDistanceRequest) -> Edited {
    let (g, rx) = start(s, r);
    let edited = s.finish_apply(g, rx.recv().expect("answer"));
    if let Edited::Show(path) = &edited {
        let (tx, rx) = mpsc::channel();
        let worker = spawn_scene(path.clone(), s.scene_token(g).expect("token"), move |v| {
            tx.send(v).expect("scene")
        });
        assert!(s.attach_scene(g, worker));
        rx.recv().expect("scene").expect("prepared");
        s.bind(Bind::Staged).expect("bind");
        assert!(s.finish_scene(g, Ok(())));
    }
    edited
}
fn reading_of(path: &Path) -> ferritecad_document::ExtrudeEditSource {
    read_extrude_source(path).expect("reading")
}
/// A request made past the form, from the accepted snapshot, by the saved UUID.
fn request(s: &Sessions, distance_mm: f64) -> EditChamferDistanceRequest {
    let source = s.export_path().expect("current");
    let reading = reading_of(&source);
    EditChamferDistanceRequest {
        source,
        expected: reading.version,
        feature: saved_of(&reading).feature,
        distance_mm,
        destination: PathBuf::new(),
    }
}
/// The shipped command line, from the same accepted file the window read.
fn peer(source: &Path, r: &EditChamferDistanceRequest, out: &Path) {
    let file = out.with_extension("json");
    std::fs::write(
        &file,
        format!(r#"{{"request_version":1,"distance_mm":{}}}"#, r.distance_mm),
    )
    .expect("request");
    cli(&[
        "edit-chamfer-distance".as_ref(),
        source.as_os_str(),
        "--feature".as_ref(),
        r.feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        reading_of(source).version.content.to_string().as_ref(),
        "--request".as_ref(),
        file.as_os_str(),
        "-o".as_ref(),
        out.as_os_str(),
    ]);
}
fn refuse(s: &mut Sessions, r: &EditChamferDistanceRequest) -> String {
    let before = state(s);
    assert_eq!(apply(s, r), Edited::Failed);
    assert_eq!(state(s), before);
    s.status.clone()
}
/// Exactly the selected Chamfer row's payload and hash changed (the stamp is
/// already set aside): every other table, row, rowid, cell, name, ref,
/// capability and extra datum is the same, and the payload keeps its UUID,
/// predecessor and joint with only the distance replaced.
fn only_distance(before: &Path, after: &Path, distance_mm: f64) {
    let chamfer = saved_of(&reading_of(before));
    let (a, b) = (sql(before), sql(after));
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    let id = rusqlite::types::Value::Blob(chamfer.feature.to_bytes().to_vec());
    for (table, (columns, rows)) in &a {
        let (other_columns, other_rows) = &b[table];
        assert_eq!(columns, other_columns, "{table}");
        assert_eq!(rows.len(), other_rows.len(), "{table}");
        let at = columns.iter().position(|c| c == "id");
        for (x, y) in rows.iter().zip(other_rows) {
            if table == "objects" && at.is_some_and(|i| x[i] == id) {
                for (i, column) in columns.iter().enumerate() {
                    if column != "payload" && column != "payload_hash" {
                        assert_eq!(x[i], y[i], "Chamfer {column}");
                    }
                }
                assert_ne!(x, y, "the Chamfer row did not change");
            } else {
                assert_eq!(x, y, "{table} changed");
            }
        }
    }
    let d = Document::open_read_only(after).expect("document");
    let ObjectPayload::Chamfer(stored) = d
        .object(chamfer.feature)
        .expect("object")
        .expect("Chamfer UUID")
        .payload
    else {
        panic!("a Chamfer");
    };
    drop(d);
    assert_eq!(stored.previous, chamfer.base_feature);
    assert_eq!(stored.edge, chamfer.edge);
    assert_eq!(stored.distance_mm, distance_mm);
}
/// Cold, cached (miss) and warm (actual archive hits) rebuilds: every saved
/// name resolves, the Body has 7 faces and the volume `(W·D − d²/2)·H`, and the
/// one named Chamfer face is the plane of exactly `corner`: outward diagonal
/// normal, through the two points `d` along each adjacent side, area `d·√2·H`.
fn cold(path: &Path, rect: [f64; 4], corner: [f64; 2], height: f64) -> f64 {
    let chamfer = saved_of(&reading_of(path));
    let distance = chamfer.distance_mm;
    let d = Document::open_read_only(path).expect("document");
    let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
    let ctx = OperationContext::default();
    let cache_root = tempfile::tempdir().expect("cache root");
    let mut cache = ferritecad_document::CacheStore::open(
        cache_root.path().join("warm.cache"),
        d.meta().document_id,
        k.identity().id(),
        k.identity().version(),
    )
    .expect("cache");
    let [x0, y0, x1, y1] = rect;
    let block = (x1 - x0) * (y1 - y0) * height;
    let exact = ((x1 - x0) * (y1 - y0) - distance * distance / 2.) * height;
    let sx = if corner[0] == x0 {
        -1.
    } else {
        assert_eq!(corner[0], x1);
        1.
    };
    let sy = if corner[1] == y0 {
        -1.
    } else {
        assert_eq!(corner[1], y1);
        1.
    };
    let refs = d.topology_refs().expect("refs");
    let named = refs
        .iter()
        .find(|r| {
            r.owner == chamfer.feature
                && matches!(r.output_role, SemanticRole::EdgeChamferFace { .. })
        })
        .expect("the Chamfer names its face");
    for pass in 0..3 {
        let built = if pass > 0 {
            ferritecad_eval::rebuild_cached(&d, &mut k, &mut cache, &ctx).map(|(built, events)| {
                if pass == 2 {
                    for feature in [chamfer.base_feature, chamfer.feature] {
                        assert!(
                            events.iter().any(|e| e.feature == feature
                                && e.outcome == ferritecad_eval::CacheOutcome::Hit),
                            "actual warm archive of {feature}: {events:?}"
                        );
                    }
                }
                built
            })
        } else {
            ferritecad_eval::rebuild_cold(&d, &mut k, &ctx)
        }
        .expect("rebuild");
        for r in &refs {
            assert!(built.resolve(r).is_ok_and(|v| !v.is_empty()), "{}", r.id);
        }
        let tip = built.shape(chamfer.body).expect("body");
        let (faces, volume) = k.shape_stats(tip).expect("stats");
        assert_eq!(faces, 7, "two caps, four sides and the chamfer face");
        assert!(
            (volume - exact).abs() < 1e-9 * block,
            "B-Rep {volume}, analytical {exact}"
        );
        let face = built.resolve(named).expect("resolve");
        assert_eq!(face.len(), 1, "one chamfer face");
        assert_eq!(
            k.face_surface(face[0]).expect("surface"),
            FaceSurface::Plane
        );
        let (origin, normal, area) = k.face_plane(face[0]).expect("plane");
        let root = 0.5f64.sqrt();
        assert!(
            (normal[0] - sx * root).abs() < 1e-9
                && (normal[1] - sy * root).abs() < 1e-9
                && normal[2].abs() < 1e-9,
            "normal {normal:?} does not point out of {corner:?}"
        );
        for p in [
            [corner[0] - sx * distance, corner[1]],
            [corner[0], corner[1] - sy * distance],
        ] {
            let off = (p[0] - origin[0]) * normal[0] + (p[1] - origin[1]) * normal[1];
            assert!(off.abs() < 1e-9, "{p:?} is {off} mm off the plane");
        }
        let beyond = (corner[0] - origin[0]) * normal[0] + (corner[1] - origin[1]) * normal[1];
        assert!(
            (beyond - distance * root).abs() < 1e-9,
            "the corner is {beyond} mm beyond the plane"
        );
        assert!(
            (area - distance * 2f64.sqrt() * height).abs() < 1e-9 * block,
            "area {area}"
        );
        built.release_all(&mut k);
    }
    exact
}
/// The STL read independently of the kernel: closed and oriented, its volume,
/// and the corner cut by one plane facing out of it whose triangles add up to
/// `d·√2·H`, with the new vertex columns `d` along each adjacent side and no
/// vertex left at the corner.
fn mesh(stl: &[u8], rect: [f64; 4], corner: [f64; 2], distance: f64, height: f64) -> f64 {
    let [x0, y0, x1, y1] = rect;
    let n = u32::from_le_bytes(stl[80..84].try_into().expect("count")) as usize;
    assert_eq!(stl.len(), 84 + 50 * n);
    let mut points = Vec::new();
    let mut volume = 0.;
    let mut flat = 0.;
    let mut edges = std::collections::BTreeMap::new();
    let key = |p: [f64; 3]| p.map(|v| (v * 1e4).round() as i64);
    let sx = if corner[0] == x0 { -1. } else { 1. };
    let sy = if corner[1] == y0 { -1. } else { 1. };
    for t in stl[84..].chunks_exact(50) {
        let mut p = [[0.; 3]; 3];
        for (i, v) in p.iter_mut().enumerate() {
            for (j, x) in v.iter_mut().enumerate() {
                let off = 12 + i * 12 + j * 4;
                *x = f32::from_le_bytes(t[off..off + 4].try_into().expect("float")) as f64;
            }
        }
        let [a, b, c] = p;
        volume += (a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]))
            / 6.;
        for (u, v) in [(a, b), (b, c), (c, a)] {
            *edges.entry((key(u), key(v))).or_insert(0) += 1;
        }
        // The cut plane at this corner: sx·(x − cx) + sy·(y − cy) = −d.
        if p.iter()
            .all(|q| (sx * (q[0] - corner[0]) + sy * (q[1] - corner[1]) + distance).abs() < 1e-4)
        {
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let m = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let length = m.iter().map(|x| x * x).sum::<f64>().sqrt();
            assert!(
                (sx * m[0] + sy * m[1]) / length / 2f64.sqrt() > 1. - 1e-4,
                "the flat faces out of the corner"
            );
            flat += length / 2.;
        }
        points.extend(p);
    }
    assert!(
        edges.values().all(|n| *n == 1) && edges.keys().all(|(u, v)| edges.contains_key(&(*v, *u))),
        "closed oriented STL"
    );
    let exact = ((x1 - x0) * (y1 - y0) - distance * distance / 2.) * height;
    assert!((volume - exact).abs() < 1e-3, "STL {volume}, exact {exact}");
    assert!(
        (flat - distance * 2f64.sqrt() * height).abs() < 1e-3,
        "flat {flat}"
    );
    let near = |x: f64, y: f64, z: f64| {
        points
            .iter()
            .any(|p| (p[0] - x).abs() < 1e-4 && (p[1] - y).abs() < 1e-4 && (p[2] - z).abs() < 1e-4)
    };
    for z in [0., height] {
        assert!(near(corner[0] - sx * distance, corner[1], z));
        assert!(near(corner[0], corner[1] - sy * distance, z));
        assert!(
            !near(corner[0], corner[1], z),
            "the cut corner kept a vertex"
        );
    }
    volume
}
fn artifacts(name: &str, stl: Vec<u8>, fbx: Vec<u8>, exact: f64, approx: f64) {
    if let Some(dir) = std::env::var_os("FCAD_CHAMFER_SESSION_ARTIFACTS") {
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
/// Height, then vertices, then three distances through the window's real
/// workers, each against the command line from the same accepted file; Undo
/// and Redo across all of it, unsaved exports, Save, cold reopen; no-op, stale,
/// refused, cancelled, stale-answer and failed-picture Apply; a branch at the
/// exact bound and Save As.
fn gate(name: &str) {
    if !native() {
        return;
    }
    let (root, source, opened) = fixture();
    let original = std::fs::read(&source).expect("source");
    let initial = saved_of(&opened);
    assert_eq!(initial.corner.corner_mm, CORNER);
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let base = initial.base_feature;
    let mut peers = source.clone();
    let mut accepted = vec![s.export_path().expect("initial")];
    let ph = root.path().join("height.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        peers.as_os_str(),
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
    assert_eq!(sql(&s.export_path().expect("height")), sql(&ph));
    peers = ph;
    accepted.push(s.export_path().expect("height"));
    let current = s.export_path().expect("current");
    let reading = reading_of(&current);
    let mut v = ferritecad_jobs::EditSketchRequest {
        source: current,
        expected: reading.version,
        sketch: initial.profile,
        vertices: reading
            .sketches
            .iter()
            .find(|c| c.sketch == initial.profile)
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
    let pv = root.path().join("vertices.fcad");
    sketch_peer(&peers, &v, root.path(), &pv);
    apply_sketch_native(&mut s, v);
    assert_eq!(sql(&s.export_path().expect("vertices")), sql(&pv));
    peers = pv;
    accepted.push(s.export_path().expect("vertices"));
    // The cut corner moved with its two Lines, found by their UUIDs.
    let rect = [-5.75, 3.25, 33., 15.5];
    let corner = [-5.75, 3.25];
    assert_eq!(
        saved_of(&reading_of(&s.export_path().expect("vertices")))
            .corner
            .corner_mm,
        corner
    );
    let mut peer_files = Vec::new();
    for (step, value) in ["3.06250", "4.4375", "6.125"].into_iter().enumerate() {
        let current = s.export_path().expect("current");
        let reading = reading_of(&current);
        let chosen = saved_of(&reading);
        let (mut form, r) = typed_distance(&current, &reading, &s, chosen.feature, value);
        let out = root.path().join(format!("peer-{step}.fcad"));
        peer(&peers, &r, &out);
        assert!(matches!(apply(&mut s, &r), Edited::Show(_)), "{}", s.status);
        form.finish_session_change();
        assert!(!form.active(), "acceptance closes the old version's form");
        let applied = s.export_path().expect("applied");
        only_distance(&current, &applied, r.distance_mm);
        assert_eq!(sql(&applied), sql(&out));
        accepted.push(applied);
        peer_files.push(out.clone());
        peers = out;
    }
    let applied = s.export_path().expect("applied");
    let (stl, fbx) = export_bytes(&applied, &source, root.path(), "unsaved");
    assert_eq!(
        (stl.clone(), fbx.clone()),
        peer_bytes(&peers, root.path(), "peer-unsaved")
    );
    let exact = cold(&applied, rect, corner, 9.25);
    let approx = mesh(&stl, rect, corner, 6.125, 9.25);
    artifacts(name, stl, fbx, exact, approx);
    assert_eq!(std::fs::read(&source).expect("source"), original);
    for expected in accepted.iter().rev().skip(1) {
        move_native(&mut s, true);
        assert_eq!(sql(&s.export_path().expect("undo")), sql(expected));
    }
    assert!(!s.dirty());
    for expected in accepted.iter().skip(1) {
        move_native(&mut s, false);
        assert_eq!(sql(&s.export_path().expect("redo")), sql(expected));
    }
    // After the history moved: the middle distance, cold and warm, and its
    // unsaved export against the command line's.
    move_native(&mut s, true);
    let middle = s.export_path().expect("middle");
    assert_eq!(saved_of(&reading_of(&middle)).distance_mm, 4.4375);
    cold(&middle, rect, corner, 9.25);
    let (stl, fbx) = export_bytes(&middle, &source, root.path(), "undo");
    assert_eq!(
        (stl.clone(), fbx),
        peer_bytes(&peer_files[1], root.path(), "peer-undo")
    );
    mesh(&stl, rect, corner, 4.4375, 9.25);
    move_native(&mut s, false);
    saved(&mut s, SaveTarget::InPlace);
    assert_eq!(sql(&source), sql(&peers));
    cold(&source, rect, corner, 9.25);
    let reopened = DocumentSession::open(&source).expect("reopen");
    assert!(!reopened.is_dirty());
    assert_eq!(sql(reopened.current().path()), sql(&source));
    drop(reopened);
    let disk = std::fs::read(&source).expect("saved bytes");
    move_native(&mut s, true);
    assert!(s.can_redo() && s.dirty());
    let before = state(&s);
    let current = s.export_path().expect("current");
    let chosen = saved_of(&reading_of(&current));
    let noop = request(&s, chosen.distance_mm);
    assert_eq!(apply(&mut s, &noop), Edited::NoChange);
    assert_eq!(state(&s), before, "a no-op kept Redo and left no file");
    let nc = root.path().join("noop.fcad");
    peer(&current, &noop, &nc);
    assert_eq!(
        sql(&current),
        sql(&nc),
        "the command line still publishes it"
    );
    let mut stale = request(&s, chosen.distance_mm + 0.5);
    stale.expected = opened.version;
    assert!(
        refuse(&mut s, &stale).contains("document changed after this form was opened"),
        "stale form guard"
    );
    // The exact bound of the stored plate, and the next number refused whole.
    let max = chosen.corner.max_distance_mm;
    assert_eq!(max, 12.25 - MIN_FLAT_MM);
    let past = request(&s, max.next_up());
    let (g, rx) = start(&mut s, &past);
    let error = rx.recv().expect("answer").expect_err("domain refusal");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
    let text = error.to_string();
    let [a, b] = chosen.edge.joint.segments();
    for needle in [
        "too large".to_owned(),
        "nothing is clamped".to_owned(),
        a.to_string(),
        b.to_string(),
        max.to_string(),
        max.next_up().to_string(),
    ] {
        assert!(text.contains(&needle), "{needle}: {text}");
    }
    assert_eq!(s.finish_apply(g, Err(error)), Edited::Failed);
    assert_eq!(state(&s), before);
    let r = request(&s, chosen.distance_mm + 0.8125);
    let (form, _) = typed_distance(
        &current,
        &reading_of(&current),
        &s,
        r.feature,
        &r.distance_mm.to_string(),
    );
    let draft = draft_state(&form);
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
    for mode in 0..5 {
        let before = state(&s);
        let (g, rx) = start(&mut s, &r);
        let produced = rx.recv().expect("answer").expect("produced");
        match mode {
            0 => {
                s.cancel();
                assert_eq!(s.finish_apply(g, Ok(produced)), Edited::Failed);
            }
            1 => {
                assert_eq!(s.finish_apply(g + 1, Ok(produced)), Edited::Ignore);
                s.finish_apply(g, Err(CadError::Cancelled));
            }
            _ => {
                assert!(matches!(s.finish_apply(g, Ok(produced)), Edited::Show(_)));
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
        assert_eq!(draft_state(&form), draft);
        assert_eq!(std::fs::read(&source).expect("source"), disk);
    }
    // A branch at the exact bound replaces Redo; Save As keeps the saved file.
    let r = request(&s, max);
    let branch_peer = root.path().join("branch-peer.fcad");
    peer(&current, &r, &branch_peer);
    assert!(matches!(apply(&mut s, &r), Edited::Show(_)), "{}", s.status);
    assert!(!s.can_redo());
    only_distance(&current, &s.export_path().expect("branch"), max);
    let branch = root.path().join("branch.fcad");
    saved(&mut s, SaveTarget::As(branch.clone()));
    assert_eq!(std::fs::read(&source).expect("source"), disk);
    assert_eq!(sql(&branch), sql(&branch_peer));
    cold(&branch, rect, corner, 9.25);
}
#[test]
fn native_height_vertices_then_distances_cross_history_and_match_cli() {
    gate("chamfer-history");
}

/// Constraints applied through the session change the solved sides and keep
/// the stored approximation; the distance is then judged on the solved plate:
/// above the stored bound it is accepted, at the solved bound too, and the next
/// number is refused by the evaluator naming the Chamfer, keeping draft and
/// Redo. A real solver conflict stays a typed `constraint` refusal.
#[test]
fn native_constraints_then_distance_use_solved_sides_and_keep_refused_draft_redo() {
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
    use ferritecad_document::{
        AddLineConstraint, AddSketchConstraint, LineConstraintKind, LineEndpoint, LineLengthMm,
        LineRelation, SketchConstraintEdits, SketchCoordinateMm,
    };
    let (root, source, reading) = fixture();
    let original = std::fs::read(&source).expect("source");
    let chosen = saved_of(&reading);
    let d = Document::open_read_only(&source).expect("document");
    let ObjectPayload::Sketch(sketch) = d
        .object(chosen.profile)
        .expect("object")
        .expect("sketch")
        .payload
    else {
        panic!("Sketch");
    };
    let lines: Vec<_> = sketch.curves.iter().map(|c| c.id).collect();
    drop(d);
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
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    apply_constraints_native(
        &mut s,
        &ferritecad_jobs::EditSketchConstraintsRequest {
            source: source.clone(),
            expected: reading.version,
            sketch: chosen.profile,
            edits: SketchConstraintEdits {
                remove: Vec::new(),
                add,
            },
            destination: PathBuf::new(),
        },
    );
    let current = s.export_path().expect("constraints");
    let dd = Document::open_read_only(&current).expect("document");
    let ObjectPayload::Sketch(after) = dd
        .object(chosen.profile)
        .expect("object")
        .expect("Sketch")
        .payload
    else {
        panic!("Sketch");
    };
    assert_eq!(after.curves, sketch.curves, "stored approximation");
    drop(dd);
    let reading = reading_of(&current);
    let stored = saved_of(&reading);
    assert!(stored.constrained);
    assert_eq!(stored.corner.corner_mm, CORNER, "a stored fact");
    let stored_max = stored.corner.max_distance_mm;
    assert_eq!(stored_max, 12.25 - MIN_FLAT_MM);
    let solved_max = 14.25 - MIN_FLAT_MM;
    // Above the stored bound, below the solved one; both routes read the same
    // accepted constrained snapshot, so no constraint UUID is mapped.
    let value = "13.5";
    assert!(value.parse::<f64>().expect("number") > stored_max);
    let (mut form, r) = typed_distance(&current, &reading, &s, stored.feature, value);
    let out = root.path().join("distance-peer.fcad");
    peer(&current, &r, &out);
    assert!(matches!(apply(&mut s, &r), Edited::Show(_)), "{}", s.status);
    form.finish_session_change();
    assert!(!form.active());
    let applied = s.export_path().expect("distance");
    only_distance(&current, &applied, 13.5);
    assert_eq!(sql(&applied), sql(&out));
    // The exact solved bound is accepted.
    let (_, r) = typed_distance(
        &applied,
        &reading_of(&applied),
        &s,
        stored.feature,
        &solved_max.to_string(),
    );
    let bound = root.path().join("bound-peer.fcad");
    peer(&applied, &r, &bound);
    assert!(matches!(apply(&mut s, &r), Edited::Show(_)), "{}", s.status);
    assert_eq!(sql(&s.export_path().expect("bound")), sql(&bound));
    // Undo/Redo crosses the constrained base; refs are cold and warm resolved.
    for _ in 0..3 {
        move_native(&mut s, true);
    }
    assert!(!s.dirty());
    for _ in 0..3 {
        move_native(&mut s, false);
    }
    let current = s.export_path().expect("bound");
    let rect = [-8., 1.25, 33., 15.5];
    let corner = [-8., 1.25];
    let exact = cold(&current, rect, corner, 6.75);
    let (stl, fbx) = export_bytes(&current, &source, root.path(), "constrained");
    assert_eq!(
        (stl.clone(), fbx.clone()),
        peer_bytes(&bound, root.path(), "peer-constrained")
    );
    let approx = mesh(&stl, rect, corner, solved_max, 6.75);
    artifacts("chamfer-constrained", stl, fbx, exact, approx);
    // With Redo available, the next number past the solved bound is refused by
    // the evaluator, through the form, which keeps the exact text.
    move_native(&mut s, true);
    assert!(s.can_redo());
    let current = s.export_path().expect("13.5");
    let reading = reading_of(&current);
    let past = solved_max.next_up().to_string();
    let (mut form, r) = typed_distance(&current, &reading, &s, stored.feature, &past);
    let draft = draft_state(&form);
    let before = state(&s);
    let (g, rx) = start(&mut s, &r);
    let error = rx.recv().expect("answer").expect_err("solved refusal");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
    let text = error.to_string();
    let [a, b] = stored.edge.joint.segments();
    for needle in [
        stored.feature.to_string(),
        "does not fit the solved plate".to_owned(),
        "too large".to_owned(),
        a.to_string(),
        b.to_string(),
        past.clone(),
        solved_max.to_string(),
    ] {
        assert!(text.contains(&needle), "{needle}: {text}");
    }
    assert_eq!(s.finish_apply(g, Err(error)), Edited::Failed);
    visible_outcome(&mut form, &s.status);
    assert!(s.status.contains(&stored.feature.to_string()));
    assert_eq!(draft_state(&form), draft);
    assert_eq!(
        state(&s),
        before,
        "the refusal kept Redo and the checkpoint"
    );
    assert_eq!(std::fs::read(&source).expect("source"), original);
    // A real conflict is the solver's typed refusal, not the Chamfer's.
    let (tx, rx) = mpsc::channel();
    let edits = SketchConstraintEdits {
        remove: Vec::new(),
        add: vec![AddSketchConstraint::Line(AddLineConstraint::Relation {
            a: lines[0],
            b: lines[1],
            relation: LineRelation::Parallel,
        })],
    };
    let g = s
        .begin_apply(|t, _, c| {
            spawn_apply_constraints(
                t,
                stored.profile,
                edits,
                reading.version,
                c.clone(),
                move |v| tx.send(v).expect("deliver"),
            )
        })
        .expect("start");
    let error = rx.recv().expect("answer").expect_err("conflict");
    assert_eq!(
        error.kind(),
        ferritecad_types::ErrorKind::Constraint,
        "{error}"
    );
    assert_eq!(s.finish_apply(g, Err(error)), Edited::Failed);
    assert_eq!(state(&s), before);
    assert_eq!(draft_state(&form), draft);
    saved(&mut s, SaveTarget::InPlace);
    assert!(s.can_redo());
    cold(&source, rect, corner, 6.75);
}
#[test]
fn stub_distance_apply_refuses_without_publication() {
    if ferritecad_occt::is_available() {
        eprintln!("skipped: requires stub");
        return;
    }
    let (_root, source, reading) = fixture();
    let original = std::fs::read(&source).expect("source");
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let (_form, r) = typed_distance(&source, &reading, &s, saved_of(&reading).feature, "3.125");
    let before = state(&s);
    let (g, rx) = start(&mut s, &r);
    let e = rx.recv().expect("answer").expect_err("no kernel");
    assert_eq!(e.kind(), ferritecad_types::ErrorKind::Unsupported);
    assert_eq!(s.finish_apply(g, Err(e)), Edited::Failed);
    assert_eq!(state(&s), before);
    assert_eq!(std::fs::read(source).expect("source"), original);
}
#[test]
fn mixed_distance_apply_uses_occt_without_solver() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        eprintln!("skipped: requires OCCT/no solver");
        return;
    }
    gate("chamfer-mixed");
}
