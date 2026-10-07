// SPDX-License-Identifier: MIT
//! §30F: existing Cut workers, all SQL cells and independent native geometry.
use super::*;
use crate::cuts::tests::session_apply::{draft_state, fixture, selected, typed_cut};
use ferritecad_document::{
    CircularCutEdit, CutExtent, ExtentVocabulary, ExtrudeEditSource, ObjectPayload, TopologyRef,
};
use ferritecad_jobs::EditCircularCutRequest;

fn start(
    s: &mut Sessions,
    r: &EditCircularCutRequest,
) -> (u64, mpsc::Receiver<Result<ProducedStep>>) {
    let (tx, rx) = mpsc::channel();
    let g = s
        .begin_apply(|t, _, c| {
            spawn_apply_cut(t, r.cut, r.edit, r.expected, c.clone(), move |v| {
                tx.send(v).expect("deliver")
            })
        })
        .expect("start");
    (g, rx)
}
pub(super) fn apply(s: &mut Sessions, r: &EditCircularCutRequest) -> Edited {
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
fn request(s: &Sessions, index: usize) -> EditCircularCutRequest {
    let source = s.export_path().expect("current");
    let reading = read_extrude_source(&source).expect("reading");
    let saved = selected(&reading, index);
    EditCircularCutRequest {
        source,
        expected: reading.version,
        cut: saved.feature,
        edit: CircularCutEdit {
            tool_curve: saved.tool_curve,
            center_mm: saved.center_mm,
            radius_mm: saved.radius_mm,
            extent: saved.extent,
            vocabulary: ExtentVocabulary::BlindOrThroughAll,
        },
        destination: PathBuf::new(),
    }
}
pub(super) fn peer(source: &Path, r: &EditCircularCutRequest, root: &Path, out: &Path) {
    let file = out.with_extension("json");
    let extent = match r.edit.extent {
        CutExtent::ThroughAll => r#"{"kind":"through_all"}"#.into(),
        CutExtent::Blind { depth_mm } => format!(r#"{{"kind":"blind","depth_mm":{depth_mm}}}"#),
    };
    std::fs::write(&file, format!(r#"{{"request_version":2,"tool_curve_id":"{}","center_mm":[{},{}],"radius_mm":{},"extent":{extent}}}"#, r.edit.tool_curve, r.edit.center_mm[0], r.edit.center_mm[1], r.edit.radius_mm)).expect("request");
    assert!(file.starts_with(root));
    cli(&[
        "edit-circular-cut".as_ref(),
        source.as_os_str(),
        "--feature".as_ref(),
        r.cut.to_string().as_ref(),
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
pub(super) fn refs(path: &Path) -> Vec<TopologyRef> {
    Document::open_read_only(path)
        .expect("document")
        .topology_refs()
        .expect("refs")
}
/// Every SQL cell, including rowid where SQLite exposes it. No payload, hash,
/// row or table is excluded. Only actually new topology_refs.id cells may map.
pub(super) fn sql(
    path: &Path,
    pairs: &[(
        ferritecad_types::StableEntityId,
        ferritecad_types::StableEntityId,
    )],
) -> std::collections::BTreeMap<String, Vec<String>> {
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
    let mut result = std::collections::BTreeMap::new();
    for table in tables {
        let quoted = table.replace('"', "\"\"");
        let mut q = db
            .prepare(&format!("SELECT rowid,* FROM \"{quoted}\""))
            .or_else(|_| db.prepare(&format!("SELECT * FROM \"{quoted}\"")))
            .expect("table");
        let columns: Vec<String> = q.column_names().into_iter().map(str::to_owned).collect();
        let rows = q
            .query_map([], |r| {
                if table == "objects" {
                    let payload = r
                        .get_ref(
                            columns
                                .iter()
                                .position(|c| c == "payload")
                                .expect("payload"),
                        )?
                        .as_blob()?;
                    let hash = r
                        .get_ref(
                            columns
                                .iter()
                                .position(|c| c == "payload_hash")
                                .expect("hash"),
                        )?
                        .as_blob()?;
                    assert_eq!(
                        ferritecad_types::ContentHash::of_bytes(payload).as_bytes(),
                        hash,
                        "raw payload hash"
                    );
                    // No new ref is embedded in an object payload: its normalized
                    // bytes and recomputed normalized hash are exactly the raw ones.
                }
                let mut row = String::new();
                for (i, column) in columns.iter().enumerate() {
                    if table == "meta" && column == "modified_at" {
                        continue;
                    }
                    let value = r.get_ref(i)?;
                    if table == "topology_refs" && column == "id" {
                        let raw = value.as_blob()?;
                        if let Some((_, mapped)) = pairs.iter().find(|(id, _)| id.to_bytes() == raw)
                        {
                            row.push_str(&format!(
                                "{column}={:?};",
                                rusqlite::types::ValueRef::Blob(&mapped.to_bytes())
                            ));
                            continue;
                        }
                    }
                    row.push_str(&format!("{column}={value:?};"));
                }
                Ok(row)
            })
            .expect("rows")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("rows");
        let mut rows = rows;
        rows.sort();
        result.insert(table, rows);
    }
    result
}
fn same_sql(ours: &Path, theirs: &Path, before: &Path) {
    let old = refs(before);
    let (a, b) = (refs(ours), refs(theirs));
    assert_eq!(a.len(), b.len());
    for r in &old {
        assert!(a.contains(r) && b.contains(r), "saved ref {} changed", r.id);
    }
    let mut pairs = Vec::new();
    for r in a.iter().filter(|r| !old.iter().any(|o| o.id == r.id)) {
        let matches: Vec<_> = b
            .iter()
            .filter(|q| {
                let mut q = (*q).clone();
                q.id = r.id;
                q == *r
            })
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "new ref role/owner/producer/links must be unique"
        );
        let peer = matches[0];
        assert!(!old.iter().any(|o| o.id == peer.id));
        pairs.push((r.id, peer.id));
    }
    assert_eq!(
        sql(ours, &pairs),
        sql(theirs, &[]),
        "every SQL cell except modified_at and explicit new ref IDs"
    );
}
pub(super) fn saved(s: &mut Sessions, target: SaveTarget) {
    let (tx, rx) = mpsc::channel();
    let g = s
        .begin_save(target, None, |p, _, c| {
            spawn_save(p, c.clone(), move |v| tx.send(v).expect("save"))
        })
        .expect("save");
    assert!(
        s.finish_save(g, rx.recv().expect("saved"))
            .expect("report")
            .published
    );
    assert!(!s.dirty());
}
fn refuse(s: &mut Sessions, r: &EditCircularCutRequest) -> String {
    let state = (
        s.export_path(),
        s.dirty(),
        s.can_undo(),
        s.can_redo(),
        s.session_saved_version(),
        private_files(s),
    );
    assert_eq!(apply(s, r), Edited::Failed);
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
pub(super) fn cold(path: &Path) -> f64 {
    let d = Document::open_read_only(path).expect("document");
    let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built =
        ferritecad_eval::rebuild_cold(&d, &mut k, &OperationContext::default()).expect("cold");
    for r in d.topology_refs().expect("refs") {
        assert!(
            built.resolve(&r).is_ok_and(|v| !v.is_empty()),
            "ref {}",
            r.id
        );
    }
    let reading = ExtrudeEditSource::read(&d).expect("reading");
    let selected = selected(&reading, 0);
    let exact = selected.boundary.area_mm2() * selected.height_mm
        - selected
            .tools
            .iter()
            .map(|t| {
                std::f64::consts::PI * t.radius_mm.powi(2) * t.extent.reach_mm(selected.height_mm)
            })
            .sum::<f64>();
    let body = d
        .objects()
        .expect("objects")
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("Body")
        .id;
    let (_, v) = k
        .shape_stats(built.shape(body).expect("shape"))
        .expect("stats");
    assert!(
        (v - exact).abs() < 1e-7 * exact,
        "B-Rep {v} vs analytical {exact}"
    );
    built.release_all(&mut k);
    d.close().expect("close");
    exact
}
pub(super) fn mesh(stl: &[u8], path: &Path) -> f64 {
    let float = |off| f32::from_le_bytes(stl[off..off + 4].try_into().expect("float")) as f64;
    let n = u32::from_le_bytes(stl[80..84].try_into().expect("count")) as usize;
    assert_eq!(stl.len(), 84 + 50 * n);
    let tris: Vec<[[f64; 3]; 3]> = (0..n)
        .map(|i| {
            std::array::from_fn(|j| {
                std::array::from_fn(|a| float(84 + 50 * i + 12 + 12 * j + 4 * a))
            })
        })
        .collect();
    let mut edges = std::collections::BTreeMap::new();
    let key = |p: [f64; 3]| p.map(|v| (v * 1e4).round() as i64);
    let mut volume = 0.;
    for [a, b, c] in &tris {
        for (u, v) in [(a, b), (b, c), (c, a)] {
            *edges.entry((key(*u), key(*v))).or_insert(0usize) += 1;
        }
        volume += a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
    }
    assert!(edges.values().all(|n| *n == 1));
    assert!(
        edges.keys().all(|(u, v)| edges.contains_key(&(*v, *u))),
        "closed oriented mesh"
    );
    volume /= 6.;
    let reading = read_extrude_source(path).expect("reading");
    let s = selected(&reading, 0);
    let exact = s.boundary.area_mm2() * s.height_mm
        - s.tools
            .iter()
            .map(|t| std::f64::consts::PI * t.radius_mm.powi(2) * t.extent.reach_mm(s.height_mm))
            .sum::<f64>();
    let upper = s.boundary.area_mm2() * s.height_mm
        - s.tools
            .iter()
            .map(|t| {
                std::f64::consts::PI * (t.radius_mm - 0.05).powi(2) * t.extent.reach_mm(s.height_mm)
            })
            .sum::<f64>();
    assert!(
        volume >= exact - 0.01 && volume <= upper + 0.01,
        "STL {volume}, exact {exact}, inscribed bound {upper}"
    );
    for tool in &s.tools {
        let wall: Vec<_> = tris
            .iter()
            .filter(|t| {
                t.iter().all(|p| {
                    ((p[0] - tool.center_mm[0]).hypot(p[1] - tool.center_mm[1]) - tool.radius_mm)
                        .abs()
                        < 1e-4
                }) && (t[0][2] - t[1][2]).abs().max((t[0][2] - t[2][2]).abs()) > 1e-4
            })
            .collect();
        assert!(wall.len() >= 12, "missing bore {}", tool.feature);
        let lo = wall
            .iter()
            .flat_map(|t| t.iter())
            .map(|p| p[2])
            .fold(f64::INFINITY, f64::min);
        let hi = wall
            .iter()
            .flat_map(|t| t.iter())
            .map(|p| p[2])
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            lo.abs() < 1e-4 && (hi - tool.extent.reach_mm(s.height_mm)).abs() < 1e-4,
            "absolute reach"
        );
        for [a, b, c] in &wall {
            let nx = (b[1] - a[1]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[1] - a[1]);
            let ny = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
            assert!(
                nx * ((a[0] + b[0] + c[0]) / 3. - tool.center_mm[0])
                    + ny * ((a[1] + b[1] + c[1]) / 3. - tool.center_mm[1])
                    < 0.,
                "hole, not boss"
            );
        }
        let covers = |z: f64| {
            tris.iter().any(|[a, b, c]| {
                if [a, b, c].iter().any(|p| (p[2] - z).abs() > 1e-4) {
                    return false;
                }
                let cross = |u: &[f64; 3], v: &[f64; 3]| {
                    (v[0] - u[0]) * (tool.center_mm[1] - u[1])
                        - (v[1] - u[1]) * (tool.center_mm[0] - u[0])
                };
                let signs = [cross(a, b), cross(b, c), cross(c, a)];
                signs.iter().all(|s| *s >= -1e-9) || signs.iter().all(|s| *s <= 1e-9)
            })
        };
        if tool.extent.leaves_a_floor(s.height_mm) {
            assert!(
                covers(tool.extent.reach_mm(s.height_mm)) && covers(s.height_mm),
                "floor and far cap"
            );
        } else {
            assert!(!covers(s.height_mm), "Through hole");
        }
    }
    volume
}
fn gate(count: usize, height: bool, name: &str) {
    if !native() {
        return;
    }
    let (root, source, opened) = fixture(count);
    let original = std::fs::read(&source).expect("source");
    cold(&source);
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let mut peer_before = source.clone();
    let mut base_steps = 0;
    if height {
        apply_native(&mut s, selected(&opened, 0).base_feature, 15.25);
        let out = root.path().join("peer-height.fcad");
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
            out.as_os_str(),
        ]);
        assert_eq!(sql(&s.export_path().expect("height"), &[]), sql(&out, &[]));
        let now = read_extrude_source(&s.export_path().expect("height")).expect("reading");
        for (a, b) in selected(&opened, 0)
            .tools
            .iter()
            .zip(selected(&now, 0).tools.iter())
        {
            assert_eq!(
                a.extent, b.extent,
                "ThroughAll intent and Blind absolute depth"
            );
        }
        peer_before = out;
        base_steps += 1;
    }
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
    base_steps += 1;
    let out = root.path().join("peer-vertices.fcad");
    sketch_peer(&peer_before, &v, root.path(), &out);
    assert_eq!(
        sql(&s.export_path().expect("vertices"), &[]),
        sql(&out, &[])
    );
    peer_before = out;
    let before_cut = s.export_path().expect("before Cut");
    let mut accepted = vec![before_cut.clone()];
    for (step, index) in [0, count / 2, count - 1].into_iter().enumerate() {
        let current = s.export_path().expect("current");
        let reading = read_extrude_source(&current).expect("reading");
        let chosen = selected(&reading, index);
        let (mut form, r) = typed_cut(
            &current,
            &reading,
            &s,
            index,
            &(chosen.center_mm[0] + 0.375).to_string(),
            &(chosen.radius_mm + 0.125).to_string(),
            None,
        );
        assert_eq!(r.cut, chosen.feature);
        assert_eq!(r.edit.tool_curve, chosen.tool_curve);
        let exact = root.path().join(format!("exact-{step}.fcad"));
        peer(&current, &r, root.path(), &exact);
        let independent = root.path().join(format!("independent-{step}.fcad"));
        peer(&peer_before, &r, root.path(), &independent);
        assert!(matches!(apply(&mut s, &r), Edited::Show(_)), "{}", s.status);
        form.finish_session_change();
        assert!(!form.active());
        let applied = s.export_path().expect("applied");
        // UUID selection is proved against every other Cut, not just count/volume.
        let changed = read_extrude_source(&applied).expect("reading");
        for old in selected(&reading, 0).tools {
            let new = selected(&changed, 0)
                .tools
                .into_iter()
                .find(|t| t.feature == old.feature)
                .expect("identity");
            if old.feature == r.cut {
                assert_eq!(new.center_mm, r.edit.center_mm);
                assert_eq!(new.radius_mm, r.edit.radius_mm);
            } else {
                assert_eq!(new, old);
            }
        }
        same_sql(&applied, &exact, &current);
        same_sql(&applied, &independent, &current);
        accepted.push(applied);
        peer_before = independent;
    }
    let applied = s.export_path().expect("applied");
    let (stl, fbx) = export_bytes(&applied, &source, root.path(), "unsaved");
    assert_eq!(
        (stl.clone(), fbx.clone()),
        peer_bytes(&peer_before, root.path(), "peer-unsaved")
    );
    let exact = cold(&applied);
    let approx = mesh(&stl, &applied);
    assert_eq!(std::fs::read(&source).expect("source"), original);
    for expected in accepted.iter().rev().skip(1) {
        move_native(&mut s, true);
        assert_eq!(
            sql(&s.export_path().expect("Undo"), &[]),
            sql(expected, &[])
        );
    }
    for _ in 0..base_steps {
        move_native(&mut s, true);
    }
    assert!(!s.dirty());
    for _ in 0..base_steps {
        move_native(&mut s, false);
    }
    for expected in accepted.iter().skip(1) {
        move_native(&mut s, false);
        assert_eq!(
            sql(&s.export_path().expect("Redo"), &[]),
            sql(expected, &[])
        );
    }
    saved(&mut s, SaveTarget::InPlace);
    same_sql(&source, &peer_before, &before_cut);
    cold(&source);
    let reopened = DocumentSession::open(&source).expect("reopen");
    assert!(!reopened.is_dirty());
    assert_eq!(sql(reopened.current().path(), &[]), sql(&source, &[]));
    drop(reopened);
    if let Some(dir) = std::env::var_os("FCAD_CUT_SESSION_ARTIFACTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("artifacts");
        std::fs::write(dir.join(format!("{name}.stl")), stl).expect("stl");
        std::fs::write(dir.join(format!("{name}.fbx")), fbx).expect("fbx");
        std::fs::write(
            dir.join(format!("{name}.metrics.txt")),
            format!("analytical_mm3={exact:.9} stl_mm3={approx:.9}\n"),
        )
        .expect("metrics");
    }
    move_native(&mut s, true);
    assert!(s.can_redo());
    let mut stale = request(&s, 0);
    stale.expected = opened.version;
    stale.edit.radius_mm += 0.25;
    assert!(
        refuse(&mut s, &stale).contains("document changed after this form was opened"),
        "stale form was not refused"
    );
    let before = (
        s.dirty(),
        s.can_undo(),
        s.can_redo(),
        s.session_saved_version(),
        private_files(&s),
    );
    let noop_request = request(&s, 0);
    assert_eq!(apply(&mut s, &noop_request), Edited::NoChange);
    assert_eq!(
        (
            s.dirty(),
            s.can_undo(),
            s.can_redo(),
            s.session_saved_version(),
            private_files(&s)
        ),
        before
    );
    let noop = root.path().join("cli-noop.fcad");
    peer(
        &s.export_path().expect("current"),
        &request(&s, 0),
        root.path(),
        &noop,
    );
    assert!(noop.exists());
    let current = s.export_path().expect("current");
    let reading = read_extrude_source(&current).expect("reading");
    let chosen = selected(&reading, 0);
    let (form, r) = typed_cut(
        &current,
        &reading,
        &s,
        0,
        &(chosen.center_mm[0] + 0.5).to_string(),
        &(chosen.radius_mm + 0.1).to_string(),
        None,
    );
    let draft = draft_state(&form);
    let saved_source = std::fs::read(&source).expect("saved source");
    for mode in 0..4 {
        let state = (
            s.export_path(),
            s.dirty(),
            s.can_undo(),
            s.can_redo(),
            s.session_saved_version(),
            private_files(&s),
        );
        let (g, rx) = start(&mut s, &r);
        let produced = rx.recv().expect("result").expect("produced");
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
            state
        );
        assert_eq!(draft_state(&form), draft);
        assert_eq!(std::fs::read(&source).expect("source"), saved_source);
    }
    let disk = std::fs::read(&source).expect("saved source");
    assert!(matches!(apply(&mut s, &r), Edited::Show(_)));
    assert!(!s.can_redo());
    let branch = root.path().join("branch.fcad");
    saved(&mut s, SaveTarget::As(branch.clone()));
    assert_eq!(std::fs::read(&source).expect("source"), disk);
    cold(&branch);
}
#[test]
fn native_height_vertices_and_first_middle_last_of_sixteen_cuts_match_cli() {
    gate(16, true, "cut-history");
}
#[test]
fn native_floor_refs_transitions_and_refusals_survive_undo_redo() {
    if !native() {
        return;
    }
    gate(1, false, "cut-single");
    let (root, source, opened) = fixture(3);
    let original = std::fs::read(&source).expect("source");
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    // Blind-through <-> ThroughAll preserves expressed intent; then through -> pocket.
    let mut r = request(&s, 0);
    r.edit.extent = CutExtent::Blind { depth_mm: 12.75 };
    assert!(matches!(apply(&mut s, &r), Edited::Show(_)));
    assert_eq!(refs(&s.export_path().expect("current")), refs(&source));
    r = request(&s, 0);
    r.edit.extent = CutExtent::ThroughAll;
    assert!(matches!(apply(&mut s, &r), Edited::Show(_)));
    let before = s.export_path().expect("before floor");
    let reading = read_extrude_source(&before).expect("reading");
    let (mut form, r) = typed_cut(&before, &reading, &s, 0, "7.5", "1.625", Some("6.125"));
    let peer_out = root.path().join("peer-floor.fcad");
    peer(&before, &r, root.path(), &peer_out);
    assert!(matches!(apply(&mut s, &r), Edited::Show(_)));
    form.finish_session_change();
    let accepted = s.export_path().expect("floor");
    same_sql(&accepted, &peer_out, &before);
    let added: Vec<_> = refs(&accepted)
        .into_iter()
        .filter(|r| !refs(&before).contains(r))
        .collect();
    assert_eq!(
        added.len(),
        3,
        "own floor plus both downstream Origin names"
    );
    cold(&accepted);
    move_native(&mut s, true);
    assert_eq!(refs(&s.export_path().expect("Undo")), refs(&before));
    move_native(&mut s, false);
    assert_eq!(
        refs(&s.export_path().expect("Redo")),
        refs(&accepted),
        "Redo must restore exact new floor UUIDs"
    );
    let mut r = request(&s, 0);
    r.edit.extent = CutExtent::ThroughAll;
    let refusal = refuse(&mut s, &r);
    for id in added.iter().map(|r| r.id) {
        assert!(
            refusal.contains(&id.to_string()),
            "protected floor UUID {id}"
        );
    }
    r = request(&s, 0);
    r.edit.center_mm = [70., 40.];
    assert!(
        refuse(&mut s, &r).contains("stay inside"),
        "actual concave outline, not bbox"
    );
    r = request(&s, 0);
    r.edit.center_mm = selected(&opened, 2).center_mm;
    let error = refuse(&mut s, &r);
    assert!(
        error.contains(&selected(&opened, 2).feature.to_string()),
        "far Cut overlap"
    );
    r = request(&s, 0);
    r.edit.tool_curve = selected(&opened, 1).tool_curve;
    assert!(refuse(&mut s, &r).contains("names a curve"));
    r = request(&s, 0);
    r.edit.extent = CutExtent::Blind { depth_mm: 13. };
    refuse(&mut s, &r);
    assert_eq!(std::fs::read(&source).expect("source"), original);
    let (stl, fbx) = export_bytes(&accepted, &source, root.path(), "floor");
    assert_eq!(
        (stl.clone(), fbx.clone()),
        peer_bytes(&peer_out, root.path(), "floor-peer")
    );
    let exact = cold(&accepted);
    let approx = mesh(&stl, &accepted);
    if let Some(dir) = std::env::var_os("FCAD_CUT_SESSION_ARTIFACTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("cut-floor.stl"), stl).expect("stl");
        std::fs::write(dir.join("cut-floor.fbx"), fbx).expect("fbx");
        std::fs::write(
            dir.join("cut-floor.metrics.txt"),
            format!("analytical_mm3={exact:.9} stl_mm3={approx:.9}\n"),
        )
        .expect("metrics");
    }
    saved(&mut s, SaveTarget::InPlace);
    cold(&source);
    assert_eq!(refs(&source), refs(&accepted));
}
#[test]
fn stub_cut_apply_refuses_without_publication() {
    if ferritecad_occt::is_available() {
        eprintln!("skipped: requires no OCCT");
        return;
    }
    let (_root, source, reading) = fixture(3);
    let bytes = std::fs::read(&source).expect("source");
    let mut s = Sessions::default();
    s.adopt(DocumentSession::open(&source).expect("session"));
    let (_form, r) = typed_cut(&source, &reading, &s, 0, "7.5", "1.625", None);
    let (g, rx) = start(&mut s, &r);
    let error = rx.recv().expect("answer").expect_err("refusal");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Unsupported);
    assert_eq!(s.finish_apply(g, Err(error)), Edited::Failed);
    assert!(!s.dirty() && !s.can_undo() && !s.can_redo());
    assert_eq!(private_files(&s).len(), 1);
    assert_eq!(std::fs::read(source).expect("source"), bytes);
}
#[test]
fn mixed_cut_apply_uses_occt_without_solver() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        eprintln!("skipped: requires OCCT without solver");
        return;
    }
    gate(3, true, "mixed-cut");
}

#[test]
fn native_compare_real_cut_gui_artifacts_with_negative_controls() {
    let Some(root) = std::env::var_os("FCAD_30F_GUI_DIR") else {
        eprintln!("skipped: requires real GUI artifacts");
        return;
    };
    assert!(native());
    let root = PathBuf::from(root);
    // No peer job may fill a missing real output, even accidentally.
    let required = [
        "after-apply.fcad",
        "after-refusal.fcad",
        "unsaved.stl",
        "unsaved.fbx",
        "undo.stl",
        "saved.fcad",
        "branch.fcad",
        "after-saveas.fcad",
    ];
    for name in required {
        assert!(root.join(name).is_file(), "missing real GUI output: {name}");
    }
    let source = root.join("source/cuts.fcad");
    let original = std::fs::read(&source).expect("source");
    assert_eq!(
        std::fs::read(root.join("after-apply.fcad")).expect("Apply source"),
        original
    );
    assert_eq!(
        std::fs::read(root.join("after-refusal.fcad")).expect("refusal source"),
        original
    );
    let tmp = tempfile::tempdir().expect("peer root");
    let reading = read_extrude_source(&source).expect("reading");
    let height = tmp.path().join("height.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        source.as_os_str(),
        "--feature".as_ref(),
        selected(&reading, 0).base_feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        reading.version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        "15.25".as_ref(),
        "-o".as_ref(),
        height.as_os_str(),
    ]);
    let reading = read_extrude_source(&height).expect("height reading");
    let base = reading
        .sketches
        .iter()
        .find(|c| c.vertices.is_some())
        .expect("base");
    let mut vertices = base.vertices.clone().expect("vertices");
    for p in &mut vertices {
        if p.start_mm[0] == -2.5 {
            p.start_mm[0] = -3.75;
        }
    }
    let v = ferritecad_jobs::EditSketchRequest {
        source: height.clone(),
        expected: reading.version,
        sketch: base.sketch,
        vertices,
        destination: PathBuf::new(),
    };
    let base = tmp.path().join("base.fcad");
    sketch_peer(&height, &v, tmp.path(), &base);
    let make = |src: &Path, index: usize, x: f64, radius: f64, depth: Option<f64>, out: &Path| {
        let reading = read_extrude_source(src).expect("reading");
        let s = selected(&reading, index);
        let r = EditCircularCutRequest {
            source: src.into(),
            expected: reading.version,
            cut: s.feature,
            edit: CircularCutEdit {
                tool_curve: s.tool_curve,
                center_mm: [x, s.center_mm[1]],
                radius_mm: radius,
                extent: depth.map_or(s.extent, |depth_mm| CutExtent::Blind { depth_mm }),
                vocabulary: ExtentVocabulary::BlindOrThroughAll,
            },
            destination: PathBuf::new(),
        };
        peer(src, &r, tmp.path(), out);
    };
    let first = tmp.path().join("first.fcad");
    make(&base, 0, 7.5, 1.625, Some(6.125), &first);
    let middle = tmp.path().join("middle.fcad");
    make(&first, 1, 43.5, 1.75, Some(5.625), &middle);
    let last = tmp.path().join("last.fcad");
    make(&middle, 2, 31.5, 2., None, &last);
    let branch = tmp.path().join("branch.fcad");
    make(&middle, 2, 31.625, 2.125, None, &branch);
    same_sql(&root.join("saved.fcad"), &last, &source);
    same_sql(&root.join("branch.fcad"), &branch, &source);
    assert_eq!(
        std::fs::read(root.join("after-saveas.fcad")).expect("previous source"),
        std::fs::read(root.join("saved.fcad")).expect("saved")
    );
    let actual = (
        std::fs::read(root.join("unsaved.stl")).expect("stl"),
        std::fs::read(root.join("unsaved.fbx")).expect("fbx"),
    );
    assert_eq!(actual, peer_bytes(&last, tmp.path(), "last-export"));
    assert_eq!(
        std::fs::read(root.join("undo.stl")).expect("Undo"),
        peer_bytes(&middle, tmp.path(), "undo-export").0
    );
    cold(&root.join("saved.fcad"));
    cold(&root.join("branch.fcad"));
    mesh(&actual.0, &root.join("saved.fcad"));
    let control = |name: &str, f: &dyn Fn()| {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err(),
            "negative control {name} accepted"
        );
    };
    control("missing", &|| {
        assert!(root.join("absent-real-output.stl").is_file());
    });
    control("original instead of save", &|| {
        same_sql(&source, &last, &source)
    });
    control("wrong chosen Cut", &|| same_sql(&middle, &last, &source));
    control("wrong branch", &|| {
        same_sql(&root.join("saved.fcad"), &branch, &source)
    });
    control("stale export", &|| {
        assert_eq!(actual, peer_bytes(&source, tmp.path(), "wrong-export"))
    });
    let corrupt = tmp.path().join("corrupt.fcad");
    std::fs::copy(root.join("saved.fcad"), &corrupt).expect("control");
    let db = rusqlite::Connection::open(&corrupt).expect("db");
    db.execute(
        "UPDATE objects SET payload_hash=zeroblob(32) WHERE id=?1",
        [selected(&reading, 0).tool_sketch.to_bytes().as_slice()],
    )
    .expect("corrupt hash");
    drop(db);
    control("raw hash", &|| same_sql(&corrupt, &last, &source));
    // New refs may map only their UUID, never owner/producer/role/link cells.
    let corrupt_ref = tmp.path().join("corrupt-ref.fcad");
    std::fs::copy(root.join("saved.fcad"), &corrupt_ref).expect("control");
    let added = refs(&corrupt_ref)
        .into_iter()
        .find(|r| {
            !refs(&source).iter().any(|old| old.id == r.id)
                && r.owner != selected(&reading, 2).feature
        })
        .expect("new floor");
    let db = rusqlite::Connection::open(&corrupt_ref).expect("db");
    db.execute(
        "UPDATE topology_refs SET owner_id=?1 WHERE id=?2",
        rusqlite::params![
            selected(&reading, 2).feature.to_bytes().as_slice(),
            added.id.to_bytes().as_slice()
        ],
    )
    .expect("wrong owner");
    drop(db);
    control("new ref owner", &|| same_sql(&corrupt_ref, &last, &source));
    println!("FCAD_30F_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true");
}
