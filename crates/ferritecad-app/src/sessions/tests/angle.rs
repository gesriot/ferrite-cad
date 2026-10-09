// SPDX-License-Identifier: MIT
//! §30E: real session workers, current snapshots, strict SQL/CLI and geometry peers.
use super::*;
use crate::sketch::tests::angle_apply::typed_angle;
use ferritecad_document::{ObjectPayload, SemanticRole};
use ferritecad_jobs::EditRevolveAngleRequest;
use ferritecad_kernel::OperationContext;

fn start(
    s: &mut Sessions,
    r: &EditRevolveAngleRequest,
) -> (Address, mpsc::Receiver<Result<ProducedStep>>) {
    let (tx, rx) = mpsc::channel();
    let g = s
        .begin_apply(|t, _, c| {
            spawn_apply_angle(t, r.feature, r.degrees, r.expected, c.clone(), move |v| {
                tx.send(v).expect("deliver")
            })
        })
        .expect("start");
    (g, rx)
}
fn apply(s: &mut Sessions, r: &EditRevolveAngleRequest) -> Edited {
    let (g, rx) = start(s, r);
    let edited = s.finish_apply(g, rx.recv().expect("edit"));
    if let Edited::Show(path) = &edited {
        let (tx, rx) = mpsc::channel();
        let c = s.scene_token(g).expect("token");
        let worker = spawn_scene(path.clone(), c, move |v| tx.send(v).expect("scene"));
        assert!(s.attach_scene(g, worker));
        rx.recv().expect("scene").expect("prepared scene");
        s.bind(Bind::Staged).expect("bind");
        assert!(s.finish_scene(g, Ok(())));
    }
    edited
}
fn peer(source: &Path, r: &EditRevolveAngleRequest, root: &Path, out: &Path) {
    let file = root.join(format!(
        "{}.json",
        out.file_stem().expect("name").to_string_lossy()
    ));
    std::fs::write(
        &file,
        format!(r#"{{"request_version":1,"angle_deg":{}}}"#, r.degrees),
    )
    .expect("request");
    cli(&[
        "edit-revolve-angle".as_ref(),
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
fn request(s: &Sessions, degrees: f64) -> EditRevolveAngleRequest {
    let source = s.export_path().expect("accepted");
    let reading = read_extrude_source(&source).expect("reading");
    EditRevolveAngleRequest {
        source,
        expected: reading.version,
        feature: reading.revolve_angles[0].feature,
        degrees,
        destination: PathBuf::new(),
    }
}
fn refuse(s: &mut Sessions, r: &EditRevolveAngleRequest) -> String {
    let before = (
        s.dirty(),
        s.can_undo(),
        s.can_redo(),
        s.session_saved_version(),
    );
    let shown = s.export_path();
    let files = private_files(s);
    assert_eq!(apply(s, r), Edited::Failed);
    assert_eq!(
        (
            s.dirty(),
            s.can_undo(),
            s.can_redo(),
            s.session_saved_version()
        ),
        before
    );
    assert_eq!(s.export_path(), shown);
    assert_eq!(private_files(s), files);
    s.status.clone()
}
fn saved(s: &mut Sessions, target: SaveTarget) {
    let (tx, rx) = mpsc::channel();
    let g = s
        .begin_save(target, None, |p, _, c| {
            spawn_save(p, c.clone(), move |v| tx.send(v).expect("saved"))
        })
        .expect("save");
    assert!(
        s.finish_save(g, rx.recv().expect("save"))
            .expect("report")
            .published
    );
    assert!(!s.dirty());
}
fn cold(path: &Path, inner: f64, outer: f64, height: f64, degrees: f64) -> f64 {
    let d = Document::open_read_only(path).expect("document");
    let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built =
        ferritecad_eval::rebuild_cold(&d, &mut k, &OperationContext::default()).expect("cold");
    let refs = d.topology_refs().expect("refs");
    assert_eq!(
        refs.iter()
            .filter(|r| matches!(r.output_role, SemanticRole::RevolveCap { .. }))
            .count(),
        2
    );
    for r in refs {
        assert!(built.resolve(&r).is_ok_and(|v| !v.is_empty()), "{}", r.id);
    }
    let body = d
        .objects()
        .expect("objects")
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("Body")
        .id;
    let shape = built.shape(body).expect("shape");
    let (_, volume) = k.shape_stats(shape).expect("stats");
    let exact = degrees.to_radians() * (outer * outer - inner * inner) * height / 2.;
    assert!(
        (volume - exact).abs() < 1e-7 * exact,
        "B-Rep {volume}, analytical {exact}"
    );
    // Independent geometric angle follows from volume / cross-sectional radial moment.
    let measured = (2. * volume / ((outer * outer - inner * inner) * height)).to_degrees();
    assert!((measured - degrees).abs() < 1e-5, "B-Rep angle {measured}");
    built.release_all(&mut k);
    exact
}
fn mesh(stl: &[u8], inner: f64, outer: f64, bottom: f64, height: f64, degrees: f64) -> f64 {
    let n = u32::from_le_bytes(stl[80..84].try_into().expect("count")) as usize;
    assert_eq!(stl.len(), 84 + 50 * n);
    let mut edges = std::collections::BTreeMap::new();
    let mut points = Vec::new();
    let mut volume = 0.;
    let key = |p: [f64; 3]| p.map(|v| (v * 1e4).round() as i64);
    for t in stl[84..].chunks_exact(50) {
        let mut p = [[0.; 3]; 3];
        for (i, v) in p.iter_mut().enumerate() {
            for (j, x) in v.iter_mut().enumerate() {
                let off = 12 + i * 12 + j * 4;
                *x = f32::from_le_bytes(t[off..off + 4].try_into().expect("f32")) as f64;
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
    assert!(edges.values().all(|v| *v == 1), "duplicate directed edge");
    assert!(
        edges.keys().all(|(u, v)| edges.contains_key(&(*v, *u))),
        "open surface"
    );
    let exact = degrees.to_radians() * (outer * outer - inner * inner) * height / 2.;
    assert!(
        volume > 0.98 * exact && volume <= 1.002 * exact,
        "STL approximation {volume}, analytical {exact}"
    );
    let radii: Vec<_> = points.iter().map(|p| p[0].hypot(p[2])).collect();
    let min = radii.iter().copied().fold(f64::INFINITY, f64::min);
    let max = radii.iter().copied().fold(0., f64::max);
    assert!((max - outer).abs() < 1e-4);
    assert!(
        (min - inner).abs() < 1e-4,
        "bore or axis closure {min}, {inner}"
    );
    let low = points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let high = points
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max);
    assert!((low - bottom).abs() < 1e-4 && (high - bottom - height).abs() < 1e-4);
    let mut angles: Vec<_> = points
        .iter()
        .filter(|p| p[0].hypot(p[2]) > 1e-4)
        .map(|p| {
            let a = (-p[2]).atan2(p[0]).to_degrees().rem_euclid(360.);
            if a > 359.9999 { 0. } else { a }
        })
        .collect();
    angles.sort_by(f64::total_cmp);
    assert!(angles[0].abs() < 1e-4);
    assert!(
        (angles[angles.len() - 1] - degrees).abs() < 1e-4,
        "mesh angle {:?}",
        angles.last()
    );
    volume
}
fn create(root: &Path, axis: bool) -> PathBuf {
    let file = root.join("create.json");
    let points = if axis {
        "[[0,-3.25],[7.5,-3.25],[7.5,9.5],[0,9.5]]"
    } else {
        "[[2.75,-3.25],[7.5,-3.25],[7.5,9.5],[2.75,9.5]]"
    };
    std::fs::write(&file, format!(r#"{{"request_version":2,"points_mm":{points},"axis":"sketch_y","extent":{{"kind":"angle","degrees":137.5}}}}"#)).expect("profile");
    let path = root.join("source.fcad");
    cli(&[
        "create-sketch-revolve".as_ref(),
        file.as_os_str(),
        "-o".as_ref(),
        path.as_os_str(),
    ]);
    path
}
fn constraint_request(path: &Path) -> ferritecad_jobs::EditSketchConstraintsRequest {
    use ferritecad_document::{
        AddLineConstraint, AddSketchConstraint, LineConstraintKind as L, LineEndpoint,
        LineLengthMm, SketchConstraintEdits, SketchCoordinateMm,
    };
    let reading = read_extrude_source(path).expect("reading");
    let choice = &reading.constraint_sketches[0];
    let curves = &choice.stored.as_ref().expect("Sketch").curves;
    let on = |i: usize, kind| {
        AddSketchConstraint::Line(AddLineConstraint::Line {
            curve: curves[i].id,
            kind,
        })
    };
    let add = vec![
        on(0, L::Horizontal),
        on(1, L::Vertical),
        on(2, L::Horizontal),
        on(3, L::Vertical),
        on(
            0,
            L::Fixed {
                at: LineEndpoint::Start,
                x: SketchCoordinateMm::new(2.75).expect("x"),
                y: SketchCoordinateMm::new(-3.25).expect("y"),
            },
        ),
        on(0, L::Distance(LineLengthMm::new(6.).expect("length"))),
        on(1, L::Distance(LineLengthMm::new(13.75).expect("height"))),
    ];
    ferritecad_jobs::EditSketchConstraintsRequest {
        source: path.to_path_buf(),
        expected: reading.version,
        sketch: choice.sketch,
        edits: SketchConstraintEdits {
            remove: vec![],
            add,
        },
        destination: PathBuf::new(),
    }
}
fn gate(axis: bool, constrained: bool, name: &str) {
    if !native() {
        return;
    }
    if constrained && !ferritecad_sketch_solver::is_available() {
        assert_ne!(
            std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
            Ok("1")
        );
        eprintln!("skipped: constrained angle needs PlaneGCS");
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = create(root.path(), axis);
    let original = std::fs::read(&source).expect("bytes");
    let private = tempfile::tempdir().expect("private");
    let mut s = Sessions::default();
    s.adopt(
        DocumentSession::open_in(private.path(), &source, HistoryLimits::default())
            .expect("session"),
    );
    let opened = read_extrude_source(&source).expect("reading");
    let original_refs = Document::open_read_only(&source)
        .expect("doc")
        .topology_refs()
        .expect("refs");
    let axis_line = opened.revolve_angles[0].axis_segment;
    assert_eq!(axis_line.is_some(), axis);
    // First step: the profile changes, leaving the user's file untouched.
    let mut vertices = opened.sketches[0].vertices.clone().expect("vertices");
    vertices[1].start_mm[0] = 8.75;
    vertices[2].start_mm[0] = 8.75;
    let v = ferritecad_jobs::EditSketchRequest {
        source: s.export_path().expect("current"),
        expected: opened.version,
        sketch: opened.sketches[0].sketch,
        vertices,
        destination: PathBuf::new(),
    };
    apply_sketch_native(&mut s, v.clone());
    let profile = s.export_path().expect("profile");
    let peer_profile = root.path().join("peer-profile.fcad");
    sketch_peer(&source, &v, root.path(), &peer_profile);
    assert_eq!(cells(&profile), cells(&peer_profile));
    let mut peer_before = peer_profile;
    let mut old_ids = Vec::new();
    let mut height = 12.75;
    if constrained {
        let r = constraint_request(&profile);
        old_ids = stored_constraints(&profile).iter().map(|c| c.id).collect();
        apply_constraints_native(&mut s, &r);
        let cp = root.path().join("peer-constraints.fcad");
        constraint_peer(&peer_before, &r, root.path(), &cp);
        same_model_with_explicit_new_ids(
            &s.export_path().expect("constraints"),
            &cp,
            v.sketch,
            &old_ids,
            "constraints cells",
        );
        peer_before = cp;
        height = 13.75;
        let read = read_extrude_source(&s.export_path().expect("current")).expect("reading");
        assert_eq!(
            read.constraint_sketches[0]
                .stored
                .as_ref()
                .expect("stored")
                .curves,
            read_extrude_source(&profile)
                .expect("profile")
                .constraint_sketches[0]
                .stored
                .as_ref()
                .expect("stored")
                .curves,
            "solved coordinates must not be stored"
        );
    }
    let before_angle = s.export_path().expect("before angle");
    let before_cells = cells(&before_angle);
    let current = read_extrude_source(&before_angle).expect("reading");
    let (mut form, r) = typed_angle(&before_angle, &current, &s, "212.25");
    assert_eq!(r.expected, current.version);
    assert!(s.dirty());
    let exact_peer = root.path().join("peer-same-current.fcad");
    peer(&before_angle, &r, root.path(), &exact_peer);
    let independent_peer = root.path().join("peer-independent.fcad");
    peer(&peer_before, &r, root.path(), &independent_peer);
    assert!(matches!(apply(&mut s, &r), Edited::Show(_)));
    form.finish_session_change();
    assert!(!form.active(), "form outlived accepted scene");
    let applied = s.export_path().expect("applied");
    assert_eq!(
        cells(&applied),
        cells(&exact_peer),
        "angle has no new UUID; every cell except modified_at"
    );
    if constrained {
        same_model_with_explicit_new_ids(
            &applied,
            &independent_peer,
            v.sketch,
            &old_ids,
            "independent chain",
        );
    } else {
        assert_eq!(cells(&applied), cells(&independent_peer));
    }
    assert_eq!(
        Document::open_read_only(&applied)
            .expect("doc")
            .topology_refs()
            .expect("refs"),
        original_refs
    );
    assert_eq!(
        read_extrude_source(&applied)
            .expect("reading")
            .revolve_angles[0]
            .axis_segment,
        axis_line
    );
    let (stl, fbx) = export_bytes(&applied, &source, root.path(), "unsaved");
    assert_eq!(
        (stl.clone(), fbx.clone()),
        peer_bytes(&exact_peer, root.path(), "exact")
    );
    let inner = if axis { 0. } else { 2.75 };
    let exact = cold(&applied, inner, 8.75, height, 212.25);
    let approximate = mesh(&stl, inner, 8.75, -3.25, height, 212.25);
    assert_eq!(std::fs::read(&source).expect("source"), original);
    move_native(&mut s, true);
    assert_eq!(cells(&s.export_path().expect("Undo")), before_cells);
    if constrained {
        move_native(&mut s, true);
        assert_eq!(
            cells(&s.export_path().expect("Undo constraints")),
            cells(&profile)
        );
    }
    move_native(&mut s, true);
    assert!(!s.dirty());
    move_native(&mut s, false);
    if constrained {
        move_native(&mut s, false);
    }
    move_native(&mut s, false);
    assert_eq!(cells(&s.export_path().expect("Redo")), cells(&applied));
    saved(&mut s, SaveTarget::InPlace);
    assert_eq!(cells(&source), cells(&exact_peer));
    cold(&source, inner, 8.75, height, 212.25);
    let reopened = DocumentSession::open_in(private.path(), &source, HistoryLimits::default())
        .expect("cold session");
    assert!(!reopened.is_dirty());
    assert_eq!(cells(reopened.current().path()), cells(&exact_peer));
    drop(reopened);
    if let Some(dir) = std::env::var_os("FCAD_ANGLE_SESSION_ARTIFACTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("artifacts");
        std::fs::write(dir.join(format!("{name}.stl")), &stl).expect("stl");
        std::fs::write(dir.join(format!("{name}.fbx")), &fbx).expect("fbx");
        std::fs::write(
            dir.join(format!("{name}.metrics.txt")),
            format!("analytical_mm3={exact:.9} stl_mm3={approximate:.9} angle_deg=212.25\n"),
        )
        .expect("measurements");
    }
    move_native(&mut s, true);
    assert!(s.can_redo());
    let stale = EditRevolveAngleRequest {
        expected: opened.version,
        ..request(&s, 198.125)
    };
    assert!(refuse(&mut s, &stale).contains("document changed after this form was opened"));
    let invalid = request(&s, 360.);
    assert!(refuse(&mut s, &invalid).contains("Could not apply"));
    // No-op still runs the reused CLI job but creates no document step or leaked file.
    let before = (
        s.dirty(),
        s.can_undo(),
        s.can_redo(),
        s.session_saved_version(),
    );
    let files = private_files(&s);
    let noop_request = request(&s, 137.5);
    assert_eq!(apply(&mut s, &noop_request), Edited::NoChange);
    assert_eq!(private_files(&s), files);
    assert_eq!(
        (
            s.dirty(),
            s.can_undo(),
            s.can_redo(),
            s.session_saved_version()
        ),
        before
    );
    let noop = root.path().join("cli-noop.fcad");
    peer(
        &s.export_path().expect("current"),
        &request(&s, 137.5),
        root.path(),
        &noop,
    );
    assert!(noop.exists());
    assert_eq!(cells(&noop), cells(&s.export_path().expect("current")));
    // Produced edit cancelled, ignored as stale answer, or GPU/scene preparation refused.
    let reading = read_extrude_source(&s.export_path().expect("current")).expect("reading");
    let (retained_form, r) =
        typed_angle(&s.export_path().expect("current"), &reading, &s, "198.125");
    let draft = crate::sketch::tests::angle_apply::draft_state(&retained_form);
    for mode in 0..3 {
        let shown = s.export_path();
        let files = private_files(&s);
        let (g, rx) = start(&mut s, &r);
        let produced = rx.recv().expect("result").expect("produced");
        if mode == 0 {
            s.cancel();
            assert_eq!(s.finish_apply(g, Ok(produced)), Edited::Failed);
        } else if mode == 1 {
            assert_eq!(s.finish_apply(g + 1, Ok(produced)), Edited::Ignore);
            s.finish_apply(g, Err(CadError::Cancelled));
        } else {
            assert!(matches!(s.finish_apply(g, Ok(produced)), Edited::Show(_)));
            s.finish_scene(g, Err(CadError::kernel("GPU preparation failed")));
        }
        assert_eq!(s.export_path(), shown);
        assert_eq!(
            crate::sketch::tests::angle_apply::draft_state(&retained_form),
            draft
        );
        assert_eq!(private_files(&s), files);
        assert_eq!(
            (
                s.dirty(),
                s.can_undo(),
                s.can_redo(),
                s.session_saved_version()
            ),
            before
        );
    }
    let new = request(&s, 198.125);
    assert!(matches!(apply(&mut s, &new), Edited::Show(_)));
    assert!(!s.can_redo());
    let branch = root.path().join("branch.fcad");
    let disk = std::fs::read(&source).expect("saved original");
    saved(&mut s, SaveTarget::As(branch.clone()));
    assert_eq!(std::fs::read(&source).expect("original"), disk);
    cold(&branch, inner, 8.75, height, 198.125);
}
#[test]
fn native_radial_profile_then_angle_is_one_session_like_cli() {
    gate(false, false, "radial");
}
#[test]
fn native_axis_closed_profile_then_angle_keeps_axis_and_caps() {
    gate(true, false, "axis");
}
#[test]
fn native_constraints_then_angle_preserve_stored_approximation_and_cli_cells() {
    gate(false, true, "constrained");
}
#[test]
fn stub_angle_apply_refuses_without_publication() {
    if ferritecad_occt::is_available() {
        eprintln!("skipped: requires a stub build");
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("source.fcad");
    crate::constraints::tests::revolve::write_sector(
        &source,
        &crate::sketch::tests::angle_apply::RADIAL,
        137.5,
        None,
    );
    let bytes = std::fs::read(&source).expect("source");
    let mut s = Sessions::default();
    s.adopt(
        DocumentSession::open_in(root.path(), &source, HistoryLimits::default()).expect("session"),
    );
    let reading = read_extrude_source(&source).expect("reading");
    let (_form, r) = typed_angle(&source, &reading, &s, "212.25");
    let (g, rx) = start(&mut s, &r);
    let error = rx.recv().expect("answer").expect_err("typed refusal");
    assert_eq!(error.kind(), ferritecad_types::ErrorKind::Unsupported);
    assert_eq!(s.finish_apply(g, Err(error)), Edited::Failed);
    assert!(!s.dirty() && !s.can_undo() && !s.can_redo());
    assert_eq!(private_files(&s).len(), 1);
    assert_eq!(std::fs::read(&source).expect("source"), bytes);
}
#[test]
fn mixed_angle_apply_needs_no_solver_until_the_profile_is_constrained() {
    if !ferritecad_occt::is_available() || ferritecad_sketch_solver::is_available() {
        eprintln!("skipped: requires OCCT without solver");
        return;
    }
    gate(false, false, "mixed-radial");
    gate(true, false, "mixed-axis");
    let root = tempfile::tempdir().expect("root");
    let source = create(root.path(), false);
    // Store constraints through the document API without asking the absent solver to solve.
    let reading = read_extrude_source(&source).expect("reading");
    let id = reading.constraint_sketches[0].sketch;
    let mut d = Document::open(&source).expect("document");
    let o = d.object(id).expect("row").expect("Sketch");
    let ObjectPayload::Sketch(mut sketch) = o.payload else {
        panic!("Sketch")
    };
    sketch
        .constraints
        .push(ferritecad_document::SketchConstraint {
            id: ferritecad_types::StableEntityId::new(),
            rule: ferritecad_document::SketchConstraintRule::Horizontal {
                a: ferritecad_document::SketchPointRef::new(
                    sketch.curves[0].id,
                    ferritecad_document::SketchPointSelector::Start,
                ),
                b: ferritecad_document::SketchPointRef::new(
                    sketch.curves[0].id,
                    ferritecad_document::SketchPointSelector::End,
                ),
            },
        });
    d.write(|w| {
        w.put_object(
            id,
            o.parent,
            o.ordinal,
            o.name.as_deref(),
            &ObjectPayload::Sketch(sketch),
        )
    })
    .expect("constraints");
    d.close().expect("close");
    let bytes = std::fs::read(&source).expect("bytes");
    let mut s = Sessions::default();
    s.adopt(
        DocumentSession::open_in(root.path(), &source, HistoryLimits::default()).expect("session"),
    );
    let r = request(&s, 212.25);
    let (g, rx) = start(&mut s, &r);
    let e = rx.recv().expect("answer").expect_err("no solver");
    assert_eq!(e.kind(), ferritecad_types::ErrorKind::Unsupported);
    assert!(e.to_string().contains("planegcs"));
    s.finish_apply(g, Err(e));
    assert!(!s.dirty() && !s.can_undo());
    assert_eq!(private_files(&s).len(), 1);
    assert_eq!(std::fs::read(&source).expect("source"), bytes);
}

/// Comparator for artifacts from a real window. This never fills a missing GUI
/// file with a CLI result. CLI peers are created only in its private directory.
#[test]
fn native_compare_real_angle_gui_artifacts_with_negative_controls() {
    let Some(directory) = std::env::var_os("FCAD_30E_GUI_DIR") else {
        eprintln!("skipped: set FCAD_30E_GUI_DIR after the real window scenario");
        return;
    };
    assert!(native(), "GUI comparison requires OCCT");
    let directory = PathBuf::from(directory);
    let files = [
        "radial-after-apply.fcad",
        "radial-unsaved.stl",
        "radial-unsaved.fbx",
        "radial-undo.stl",
        "radial-saved.fcad",
        "radial-after-saveas.fcad",
        "radial-branch.fcad",
        "axis-unsaved.stl",
        "axis-unsaved.fbx",
        "axis-saved.fcad",
        "constrained-after-refusal.fcad",
        "constrained-unsaved.stl",
        "constrained-unsaved.fbx",
        "constrained-saved.fcad",
    ];
    let require = |root: &Path| {
        for name in files {
            assert!(
                root.join(name).is_file(),
                "FCAD_30E_GUI_COMPARE_MISSING {name}"
            );
        }
    };
    require(&directory);
    let root = tempfile::tempdir().expect("peer root");
    let bytes = |name: &str| std::fs::read(directory.join(name)).expect("GUI artifact");
    let control = |name: &str, check: &dyn Fn()| {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(check)).is_err(),
            "negative control passed: {name}"
        );
        eprintln!("FCAD_30E_GUI_CONTROL_REJECTED {name}");
    };
    control("missing output", &|| require(root.path()));
    let mut expected_radial = PathBuf::new();
    let mut radial_profile = PathBuf::new();
    for (name, axis) in [("radial", false), ("axis", true)] {
        let source = directory.join("source").join(format!("{name}.fcad"));
        let reading = read_extrude_source(&source).expect("source");
        let choice = &reading.sketches[0];
        let mut vertices = choice.vertices.clone().expect("vertices");
        vertices[1].start_mm[0] = 8.75;
        vertices[2].start_mm[0] = 8.75;
        let r = ferritecad_jobs::EditSketchRequest {
            source: source.clone(),
            expected: reading.version,
            sketch: choice.sketch,
            vertices,
            destination: PathBuf::new(),
        };
        let profile = root.path().join(format!("{name}-profile.fcad"));
        sketch_peer(&source, &r, root.path(), &profile);
        let angle_reading = read_extrude_source(&profile).expect("profile");
        let a = EditRevolveAngleRequest {
            source: profile.clone(),
            expected: angle_reading.version,
            feature: angle_reading.revolve_angles[0].feature,
            degrees: 212.25,
            destination: PathBuf::new(),
        };
        let expected = root.path().join(format!("{name}-angle.fcad"));
        peer(&profile, &a, root.path(), &expected);
        let saved = directory.join(format!("{name}-saved.fcad"));
        assert_eq!(cells(&saved), cells(&expected), "GUI Save cells");
        let (stl, fbx) = peer_bytes(&expected, root.path(), name);
        assert_eq!(bytes(&format!("{name}-unsaved.stl")), stl);
        assert_eq!(bytes(&format!("{name}-unsaved.fbx")), fbx);
        let inner = if axis { 0. } else { 2.75 };
        cold(&saved, inner, 8.75, 12.75, 212.25);
        mesh(&stl, inner, 8.75, -3.25, 12.75, 212.25);
        assert_eq!(
            Document::open_read_only(&source)
                .expect("source")
                .topology_refs()
                .expect("refs"),
            Document::open_read_only(&saved)
                .expect("saved")
                .topology_refs()
                .expect("refs")
        );
        if !axis {
            expected_radial = expected;
            radial_profile = profile;
        }
    }
    assert_eq!(
        bytes("radial-after-apply.fcad"),
        bytes("source/radial.fcad"),
        "Apply wrote user's file"
    );
    assert_eq!(
        bytes("constrained-after-refusal.fcad"),
        bytes("source/constrained.fcad"),
        "refusal wrote user's file"
    );
    assert_eq!(
        bytes("radial-after-saveas.fcad"),
        bytes("radial-saved.fcad"),
        "Save As changed original"
    );
    let (stl, _) = peer_bytes(&radial_profile, root.path(), "undo");
    assert_eq!(bytes("radial-undo.stl"), stl);
    let reading = read_extrude_source(&radial_profile).expect("profile");
    let branch = root.path().join("branch.fcad");
    let r = EditRevolveAngleRequest {
        source: radial_profile.clone(),
        expected: reading.version,
        feature: reading.revolve_angles[0].feature,
        degrees: 198.125,
        destination: PathBuf::new(),
    };
    peer(&radial_profile, &r, root.path(), &branch);
    assert_eq!(cells(&directory.join("radial-branch.fcad")), cells(&branch));
    cold(
        &directory.join("radial-branch.fcad"),
        2.75,
        8.75,
        12.75,
        198.125,
    );

    use ferritecad_document::{
        AddLineConstraint, AddSketchConstraint, LineConstraintKind, LineLengthMm,
        SketchConstraintRule,
    };
    let source = directory.join("source/constrained.fcad");
    let reading = read_extrude_source(&source).expect("constrained");
    let choice = &reading.constraint_sketches[0];
    let stored = choice.stored.as_ref().expect("stored");
    let curve = stored.curves[1].id;
    let old = stored
        .constraints
        .iter()
        .find(|c| matches!(c.rule, SketchConstraintRule::Distance { a, .. } if a.curve==curve))
        .expect("height")
        .id;
    let r = ferritecad_jobs::EditSketchConstraintsRequest {
        source: source.clone(),
        expected: reading.version,
        sketch: choice.sketch,
        edits: ferritecad_document::SketchConstraintEdits {
            remove: vec![old],
            add: vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                curve,
                kind: LineConstraintKind::Distance(LineLengthMm::new(13.75).expect("height")),
            })],
        },
        destination: PathBuf::new(),
    };
    let cp = root.path().join("constrained.fcad");
    constraint_peer(&source, &r, root.path(), &cp);
    let reading = read_extrude_source(&cp).expect("constraint peer");
    let a = EditRevolveAngleRequest {
        source: cp.clone(),
        expected: reading.version,
        feature: reading.revolve_angles[0].feature,
        degrees: 212.25,
        destination: PathBuf::new(),
    };
    let ca = root.path().join("constrained-angle.fcad");
    peer(&cp, &a, root.path(), &ca);
    let saved = directory.join("constrained-saved.fcad");
    let old_ids: Vec<_> = stored.constraints.iter().map(|c| c.id).collect();
    let pairs = same_model_with_explicit_new_ids(
        &saved,
        &ca,
        choice.sketch,
        &old_ids,
        "GUI constraint/angle cells",
    );
    assert_eq!(pairs.len(), 1, "only replacement length gets a new UUID");
    let after = read_extrude_source(&saved).expect("saved");
    assert_eq!(
        after.constraint_sketches[0]
            .stored
            .as_ref()
            .expect("stored")
            .curves,
        stored.curves,
        "solver baked coordinates"
    );
    let (stl, fbx) = peer_bytes(&ca, root.path(), "constrained");
    assert_eq!(bytes("constrained-unsaved.stl"), stl);
    assert_eq!(bytes("constrained-unsaved.fbx"), fbx);
    cold(&saved, 2.75, 7.5, 13.75, 212.25);
    mesh(&stl, 2.75, 7.5, -3.25, 13.75, 212.25);
    control("old saved angle", &|| {
        assert_eq!(
            cells(&directory.join("radial-saved.fcad")),
            cells(&directory.join("source/radial.fcad"))
        )
    });
    control("wrong branch", &|| {
        assert_eq!(
            cells(&directory.join("radial-branch.fcad")),
            cells(&expected_radial)
        )
    });
    let (disk_stl, _) = peer_bytes(&directory.join("source/radial.fcad"), root.path(), "disk");
    control("export read disk", &|| {
        assert_eq!(bytes("radial-unsaved.stl"), disk_stl)
    });
    control("wrong mesh angle", &|| {
        mesh(&stl, 2.75, 7.5, -3.25, 13.75, 198.125);
    });
    let changed = root.path().join("renamed.fcad");
    std::fs::copy(&saved, &changed).expect("control copy");
    let db = rusqlite::Connection::open(&changed).expect("control db");
    db.execute(
        "UPDATE objects SET name='wrong Sketch' WHERE id=?1",
        [choice.sketch.to_bytes().as_slice()],
    )
    .expect("rename");
    drop(db);
    control("Sketch metadata", &|| {
        same_model_with_explicit_new_ids(
            &changed,
            &ca,
            choice.sketch,
            &old_ids,
            "negative metadata",
        );
    });
    let changed = root.path().join("corrupt-hash.fcad");
    std::fs::copy(&saved, &changed).expect("control copy");
    let db = rusqlite::Connection::open(&changed).expect("control db");
    db.execute(
        "UPDATE objects SET payload_hash=zeroblob(32) WHERE id=?1",
        [choice.sketch.to_bytes().as_slice()],
    )
    .expect("hash");
    drop(db);
    control("payload hash", &|| {
        same_model_with_explicit_new_ids(&changed, &ca, choice.sketch, &old_ids, "negative hash");
    });
    eprintln!("FCAD_30E_GUI_COMPARE_OK negative_controls=7 all_SQL_cells=true");
}
