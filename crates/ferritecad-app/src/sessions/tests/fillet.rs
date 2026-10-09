// SPDX-License-Identifier: MIT
//! §30G: real workers, UUID selection, exact SQL and independent geometry peers.
use super::*;
use crate::fillets::tests::session_apply::{
    draft_state, fixture, selected, typed_radius, visible_outcome,
};
use ferritecad_document::{ObjectPayload, SavedFillet};
use ferritecad_jobs::EditFilletRadiusRequest;
use ferritecad_kernel::GeometryKernel;

fn start(
    s: &mut Sessions,
    r: &EditFilletRadiusRequest,
) -> (Address, mpsc::Receiver<Result<ProducedStep>>) {
    let (tx, rx) = mpsc::channel();
    let g = s
        .begin_apply(|t, _, c| {
            spawn_apply_fillet_radius(t, r.feature, r.radius_mm, r.expected, c.clone(), move |v| {
                tx.send(v).expect("deliver")
            })
        })
        .expect("start");
    (g, rx)
}
pub(super) fn apply(s: &mut Sessions, r: &EditFilletRadiusRequest) -> Edited {
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
fn request(s: &Sessions, index: usize, radius_mm: f64) -> EditFilletRadiusRequest {
    let source = s.export_path().expect("current");
    let reading = read_extrude_source(&source).expect("reading");
    EditFilletRadiusRequest {
        source,
        expected: reading.version,
        feature: selected(&reading, index).feature,
        radius_mm,
        destination: PathBuf::new(),
    }
}
pub(super) fn peer(source: &Path, r: &EditFilletRadiusRequest, out: &Path) {
    let file = out.with_extension("json");
    std::fs::write(
        &file,
        format!(r#"{{"request_version":1,"radius_mm":{}}}"#, r.radius_mm),
    )
    .expect("request");
    cli(&[
        "edit-fillet-radius".as_ref(),
        source.as_os_str(),
        "--feature".as_ref(),
        r.feature.to_string().as_ref(),
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
    ]);
}
// Every table/row/cell and SQLite rowid; only modified_at is set aside. There
// are no new identities in this operation and no UUID normalization anywhere.
pub(super) fn sql(
    path: &Path,
) -> std::collections::BTreeMap<String, (Vec<String>, Vec<Vec<rusqlite::types::Value>>)> {
    let mut all = crate::fillets::tests::tables(path);
    for (table, (columns, rows)) in &mut all {
        for row in rows {
            if let (Some(p), Some(h)) = (
                columns.iter().position(|c| c == "payload"),
                columns.iter().position(|c| c == "payload_hash"),
            ) {
                let (rusqlite::types::Value::Blob(payload), rusqlite::types::Value::Blob(hash)) =
                    (&row[p], &row[h])
                else {
                    panic!("raw payload");
                };
                assert_eq!(
                    ferritecad_types::ContentHash::of_bytes(payload).as_bytes(),
                    hash.as_slice(),
                    "raw {table} hash"
                );
            }
            if table == "meta" {
                row[columns
                    .iter()
                    .position(|c| c == "modified_at")
                    .expect("stamp")] = rusqlite::types::Value::Null;
            }
        }
    }
    all
}
pub(super) fn saved(s: &mut Sessions, target: SaveTarget) {
    let (tx, rx) = mpsc::channel();
    let g = s
        .begin_save(target, None, |p, _, c| {
            spawn_save(p, c.clone(), move |v| tx.send(v).expect("save"))
        })
        .expect("start");
    assert!(
        s.finish_save(g, rx.recv().expect("save"))
            .expect("report")
            .published
    );
    assert!(!s.dirty());
}
pub(super) fn state(
    s: &Sessions,
) -> (
    Option<PathBuf>,
    bool,
    bool,
    bool,
    Option<ferritecad_document::DocumentVersion>,
    Vec<String>,
) {
    (
        s.export_path(),
        s.dirty(),
        s.can_undo(),
        s.can_redo(),
        s.session_saved_version(),
        private_files(s),
    )
}
fn refuse(s: &mut Sessions, r: &EditFilletRadiusRequest) -> String {
    let before = state(s);
    assert_eq!(apply(s, r), Edited::Failed);
    assert_eq!(state(s), before);
    s.status.clone()
}
pub(super) fn history(path: &Path) -> Vec<SavedFillet> {
    let reading = read_extrude_source(path).expect("reading");
    (0..reading.fillet_features.len())
        .map(|i| selected(&reading, i))
        .collect()
}
pub(super) fn cold(path: &Path, width: f64, depth: f64, height: f64) -> f64 {
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
    let exact = width * depth * height
        - (1. - std::f64::consts::PI / 4.)
            * history(path)
                .iter()
                .map(|f| f.radius_mm.powi(2))
                .sum::<f64>()
            * height;
    for pass in 0..3 {
        let built = if pass > 0 {
            ferritecad_eval::rebuild_cached(&d, &mut k, &mut cache, &ctx).map(|(built, events)| {
                if pass == 2 {
                    assert!(
                        events
                            .iter()
                            .filter(|e| e.outcome == ferritecad_eval::CacheOutcome::Hit)
                            .count()
                            > history(path).len(),
                        "actual warm archives"
                    );
                }
                built
            })
        } else {
            ferritecad_eval::rebuild_cold(&d, &mut k, &ctx)
        }
        .expect("rebuild");
        for r in d.topology_refs().expect("refs") {
            assert!(built.resolve(&r).is_ok_and(|v| !v.is_empty()), "{}", r.id);
        }
        let body = history(path)[0].body;
        let (_, volume) = k
            .shape_stats(built.shape(body).expect("body"))
            .expect("stats");
        assert!(
            (volume - exact).abs() < 1e-7 * exact,
            "B-Rep {volume}, analytical {exact}"
        );
        built.release_all(&mut k);
    }
    exact
}
pub(super) fn mesh(stl: &[u8], fillets: &[SavedFillet], bounds: [f64; 4], height: f64) -> f64 {
    let [x0, y0, x1, y1] = bounds;
    let n = u32::from_le_bytes(stl[80..84].try_into().expect("count")) as usize;
    assert_eq!(stl.len(), 84 + 50 * n);
    let mut points = Vec::new();
    let mut volume = 0.;
    let mut edges = std::collections::BTreeMap::new();
    let key = |p: [f64; 3]| p.map(|v| (v * 1e4).round() as i64);
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
        points.extend(p);
    }
    assert!(
        edges.values().all(|n| *n == 1) && edges.keys().all(|(u, v)| edges.contains_key(&(*v, *u))),
        "closed oriented STL"
    );
    let exact = (x1 - x0) * (y1 - y0) * height
        - (1. - std::f64::consts::PI / 4.)
            * fillets.iter().map(|f| f.radius_mm.powi(2)).sum::<f64>()
            * height;
    assert!(
        volume > 0.995 * exact && volume < 1.005 * exact,
        "STL {volume}, exact {exact}"
    );
    // A per-corner quarter-circle fit distinguishes different same-named Fillets
    // even when swapping radii would preserve the total volume.
    for f in fillets {
        let [x, y] = f.corner.corner_mm;
        let r = f.radius_mm;
        let cx = if (x - x0).abs() < 1e-5 {
            x0 + r
        } else {
            x1 - r
        };
        let cy = if (y - y0).abs() < 1e-5 {
            y0 + r
        } else {
            y1 - r
        };
        let arc: Vec<_> = points
            .iter()
            .filter(|p| (p[0] - x).abs() <= r + 1e-5 && (p[1] - y).abs() <= r + 1e-5)
            .collect();
        assert!(arc.len() >= 6, "corner {} has no arc", f.feature);
        for p in arc {
            assert!(
                ((p[0] - cx).hypot(p[1] - cy) - r).abs() < 1e-4,
                "wrong radius/corner {}",
                f.feature
            );
        }
    }
    volume
}
fn artifacts(name: &str, stl: Vec<u8>, fbx: Vec<u8>, exact: f64, approx: f64) {
    if let Some(dir) = std::env::var_os("FCAD_FILLET_SESSION_ARTIFACTS") {
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
fn gate(count: usize, name: &str) {
    if !native() {
        return;
    }
    let (root, source, opened) = fixture(count);
    let original = std::fs::read(&source).expect("source");
    let initial = history(&source);
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let base = initial[0].base_feature;
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
    let reading = read_extrude_source(&current).expect("reading");
    let mut v = ferritecad_jobs::EditSketchRequest {
        source: current,
        expected: reading.version,
        sketch: initial[0].profile,
        vertices: reading
            .sketches
            .iter()
            .find(|c| c.sketch == initial[0].profile)
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
    let indices = if count == 1 { vec![0] } else { vec![0, 1, 3] };
    for (step, index) in indices.into_iter().enumerate() {
        let current = s.export_path().expect("current");
        let reading = read_extrude_source(&current).expect("reading");
        let chosen = selected(&reading, index);
        let radius = chosen.radius_mm + 0.375;
        let (mut form, r) =
            typed_radius(&current, &reading, &s, chosen.feature, &radius.to_string());
        let out = root.path().join(format!("peer-{step}.fcad"));
        peer(&peers, &r, &out);
        assert!(matches!(apply(&mut s, &r), Edited::Show(_)), "{}", s.status);
        form.finish_session_change();
        assert!(!form.active());
        let applied = s.export_path().expect("applied");
        for old in history(&current) {
            let new = history(&applied)
                .into_iter()
                .find(|f| f.feature == old.feature)
                .expect("UUID");
            if old.feature == r.feature {
                assert_eq!(new.radius_mm, r.radius_mm, "selected Fillet");
            } else {
                assert_eq!(new.radius_mm, old.radius_mm, "other Fillet");
            }
            assert_eq!(new.edge, old.edge);
            assert_eq!(new.previous, old.previous);
        }
        assert_eq!(sql(&applied), sql(&out));
        accepted.push(applied);
        peers = out;
    }
    let applied = s.export_path().expect("applied");
    let (stl, fbx) = export_bytes(&applied, &source, root.path(), "unsaved");
    assert_eq!(
        (stl.clone(), fbx.clone()),
        peer_bytes(&peers, root.path(), "peer-unsaved")
    );
    let exact = cold(&applied, 38.75, 12.25, 9.25);
    let approx = mesh(&stl, &history(&applied), [-5.75, 3.25, 33., 15.5], 9.25);
    artifacts(name, stl, fbx, exact, approx);
    assert_eq!(std::fs::read(&source).expect("source"), original);
    for expected in accepted.iter().rev().skip(1) {
        move_native(&mut s, true);
        assert_eq!(sql(&s.export_path().expect("undo")), sql(expected));
        cold(
            &s.export_path().expect("undo"),
            if s.session.as_ref().expect("session").undo_depth() >= 2 {
                38.75
            } else {
                37.5
            },
            12.25,
            if s.can_undo() { 9.25 } else { 6.75 },
        );
    }
    assert!(!s.dirty());
    for expected in accepted.iter().skip(1) {
        move_native(&mut s, false);
        assert_eq!(sql(&s.export_path().expect("redo")), sql(expected));
    }
    saved(&mut s, SaveTarget::InPlace);
    assert_eq!(sql(&source), sql(&peers));
    cold(&source, 38.75, 12.25, 9.25);
    let reopened = DocumentSession::open(&source).expect("reopen");
    assert!(!reopened.is_dirty());
    assert_eq!(sql(reopened.current().path()), sql(&source));
    drop(reopened);
    move_native(&mut s, true);
    let before = state(&s);
    let current = s.export_path().expect("current");
    let chosen = history(&current)[0].clone();
    let noop = request(&s, 0, chosen.radius_mm);
    assert_eq!(apply(&mut s, &noop), Edited::NoChange);
    assert_eq!(state(&s), before);
    let nc = root.path().join("noop.fcad");
    peer(&current, &noop, &nc);
    assert_eq!(sql(&current), sql(&nc));
    let mut stale = request(&s, 0, chosen.radius_mm + 0.125);
    stale.expected = opened.version;
    assert!(
        refuse(&mut s, &stale).contains("document changed after this form was opened"),
        "stale form guard"
    );
    let bad = request(&s, 0, 6.25);
    assert!(refuse(&mut s, &bad).contains("too large"));
    let r = request(&s, 0, chosen.radius_mm + 0.125);
    let reading = read_extrude_source(&current).expect("reading");
    let (form, _) = typed_radius(&current, &reading, &s, r.feature, &r.radius_mm.to_string());
    let draft = draft_state(&form);
    let disk = std::fs::read(&source).expect("disk");
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
        assert_eq!(state(&s), before);
        assert_eq!(draft_state(&form), draft);
        assert_eq!(std::fs::read(&source).expect("source"), disk);
    }
    let branch_peer = root.path().join("branch-peer.fcad");
    peer(&current, &r, &branch_peer);
    assert!(matches!(apply(&mut s, &r), Edited::Show(_)));
    assert!(!s.can_redo());
    let branch = root.path().join("branch.fcad");
    saved(&mut s, SaveTarget::As(branch.clone()));
    assert_eq!(std::fs::read(&source).expect("source"), disk);
    assert_eq!(sql(&branch), sql(&branch_peer));
    cold(&branch, 38.75, 12.25, 9.25);
}
#[test]
fn native_one_and_four_fillets_cross_base_history_and_match_cli() {
    gate(1, "fillet-single");
    gate(4, "fillet-history");
}

#[test]
fn native_constraints_then_radius_use_solved_rectangle_and_keep_refused_draft_redo() {
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
        SketchConstraintEdits, SketchCoordinateMm,
    };
    let (root, source, reading) = fixture(4);
    let original = std::fs::read(&source).expect("source");
    let chosen = selected(&reading, 0);
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
    let cr = ferritecad_jobs::EditSketchConstraintsRequest {
        source: source.clone(),
        expected: reading.version,
        sketch: chosen.profile,
        edits: SketchConstraintEdits {
            remove: Vec::new(),
            add,
        },
        destination: PathBuf::new(),
    };
    apply_constraints_native(&mut s, &cr);
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
    let (mut form, r) = typed_radius(
        &current,
        &read_extrude_source(&current).expect("reading"),
        &s,
        chosen.feature,
        "7.125",
    );
    let out = root.path().join("radius-peer.fcad");
    peer(&current, &r, &out);
    assert!(matches!(apply(&mut s, &r), Edited::Show(_)), "{}", s.status);
    form.finish_session_change();
    assert!(!form.active());
    assert_eq!(sql(&s.export_path().expect("radius")), sql(&out));
    // Undo/Redo crosses the constrained base; refs are cold and warm resolved.
    move_native(&mut s, true);
    move_native(&mut s, true);
    assert!(!s.dirty());
    move_native(&mut s, false);
    move_native(&mut s, false);
    let current = s.export_path().expect("radius");
    let exact = cold(&current, 41., 14.25, 6.75);
    let (stl, fbx) = export_bytes(&current, &source, root.path(), "constrained");
    assert_eq!(
        (stl.clone(), fbx.clone()),
        peer_bytes(&out, root.path(), "peer-constrained")
    );
    // Use solved corner coordinates with the original Line UUIDs in the mesh fit.
    let mut solved = history(&current);
    for f in &mut solved {
        let [x, y] = f.corner.corner_mm;
        f.corner.corner_mm = [
            if x == 33. { 33. } else { -8. },
            if y == 15.5 { 15.5 } else { 1.25 },
        ];
    }
    let approx = mesh(&stl, &solved, [-8., 1.25, 33., 15.5], 6.75);
    artifacts("fillet-constrained", stl, fbx, exact, approx);
    // Make Redo available while keeping the large earlier radius in the model.
    let other = request(&s, 1, 3.25);
    assert!(matches!(apply(&mut s, &other), Edited::Show(_)));
    move_native(&mut s, true);
    assert!(s.can_redo());
    for (index, value, need) in [(0, "7.25", "too large"), (2, "7.125", "Line")] {
        let current = s.export_path().expect("current");
        let reading = read_extrude_source(&current).expect("reading");
        let saved = selected(&reading, index);
        let (mut form, r) = typed_radius(&current, &reading, &s, saved.feature, value);
        let draft = draft_state(&form);
        let before = (
            s.export_path(),
            s.dirty(),
            s.can_redo(),
            s.session_saved_version(),
        );
        let (g, rx) = start(&mut s, &r);
        let error = rx.recv().expect("answer").expect_err("solved refusal");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
        let text = error.to_string();
        assert!(text.contains(need), "{text}");
        if index == 2 {
            assert!(text.contains(&chosen.feature.to_string()), "{text}");
            let [a, b] = saved.edge.joint.segments();
            assert!(
                text.contains(&a.to_string()) || text.contains(&b.to_string()),
                "{text}"
            );
        }
        assert_eq!(s.finish_apply(g, Err(error)), Edited::Failed);
        visible_outcome(&mut form, &s.status);
        assert_eq!(draft_state(&form), draft);
        assert_eq!(
            (
                s.export_path(),
                s.dirty(),
                s.can_redo(),
                s.session_saved_version()
            ),
            before
        );
        assert_eq!(std::fs::read(&source).expect("source"), original);
    }
    saved(&mut s, SaveTarget::InPlace);
    cold(&source, 41., 14.25, 6.75);
    let disk = std::fs::read(&source).expect("saved bytes");
    // A solved square makes both adjacent sides of the fourth corner limiting.
    // Refuse against the earlier bottom neighbour, then against the other one.
    let current = s.export_path().expect("current");
    let d = Document::open_read_only(&current).expect("document");
    let ObjectPayload::Sketch(sketch) = d
        .object(chosen.profile)
        .expect("object")
        .expect("Sketch")
        .payload
    else {
        panic!("Sketch");
    };
    let width = sketch
        .constraints
        .iter()
        .find(|c| {
            matches!(&c.rule,
        ferritecad_document::SketchConstraintRule::Distance { a, .. } if a.curve == lines[1])
        })
        .expect("horizontal length")
        .id;
    drop(d);
    let square = ferritecad_jobs::EditSketchConstraintsRequest {
        source: current.clone(),
        expected: read_extrude_source(&current).expect("reading").version,
        sketch: chosen.profile,
        edits: SketchConstraintEdits {
            remove: vec![width],
            add: vec![line(
                lines[1],
                LineConstraintKind::Distance(LineLengthMm::new(14.25).expect("width")),
            )],
        },
        destination: PathBuf::new(),
    };
    apply_constraints_native(&mut s, &square);
    for neighbour in [0, 1] {
        if neighbour == 1 {
            let r = request(&s, 0, 0.25);
            assert!(matches!(apply(&mut s, &r), Edited::Show(_)));
            let r = request(&s, 1, 7.125);
            assert!(matches!(apply(&mut s, &r), Edited::Show(_)));
        }
        let r = request(&s, 2, 1.75);
        assert!(matches!(apply(&mut s, &r), Edited::Show(_)));
        move_native(&mut s, true);
        let current = s.export_path().expect("square");
        let reading = read_extrude_source(&current).expect("reading");
        let fourth = selected(&reading, 3);
        let beside = selected(&reading, neighbour);
        let shared = fourth
            .neighbours
            .iter()
            .find(|n| n.feature == beside.feature)
            .expect("neighbour")
            .shared
            .expect("shared Line")
            .0;
        let (mut form, r) = typed_radius(&current, &reading, &s, fourth.feature, "7.125");
        let draft = draft_state(&form);
        let reason = refuse(&mut s, &r);
        assert!(
            reason.contains(&beside.feature.to_string()) && reason.contains(&shared.to_string()),
            "{reason}"
        );
        visible_outcome(&mut form, &reason);
        assert_eq!(draft_state(&form), draft);
        assert!(s.can_redo());
        assert_eq!(std::fs::read(&source).expect("source"), disk);
    }
}
#[test]
fn stub_radius_apply_refuses_without_publication() {
    if ferritecad_occt::is_available() {
        eprintln!("skipped: requires stub");
        return;
    }
    let (_root, source, reading) = fixture(1);
    let original = std::fs::read(&source).expect("source");
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let (_form, r) = typed_radius(
        &source,
        &reading,
        &s,
        selected(&reading, 0).feature,
        "3.125",
    );
    let before = state(&s);
    let (g, rx) = start(&mut s, &r);
    let e = rx.recv().expect("answer").expect_err("no kernel");
    assert_eq!(e.kind(), ferritecad_types::ErrorKind::Unsupported);
    assert_eq!(s.finish_apply(g, Err(e)), Edited::Failed);
    assert_eq!(state(&s), before);
    assert_eq!(std::fs::read(source).expect("source"), original);
}
#[test]
fn mixed_radius_apply_uses_occt_without_solver() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        eprintln!("skipped: requires OCCT/no solver");
        return;
    }
    gate(4, "fillet-mixed");
}
