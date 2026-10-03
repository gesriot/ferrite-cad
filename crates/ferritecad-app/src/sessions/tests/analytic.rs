// SPDX-License-Identifier: MIT
//! §30D: a saved Circle or annulus applied inside the open document, on the real
//! workers and against the real peer command line. Nothing here introduces a new
//! identity, so the saved document is compared with the command line's copy in
//! every SQL cell: no Sketch row and no payload is set aside.

use super::*;
use crate::sketch::tests::analytic_apply::{typed_annulus, typed_circle, write_round};

enum Request {
    Circle(ferritecad_jobs::EditCircleRequest),
    Annulus(ferritecad_jobs::EditAnnulusRequest),
}

impl Request {
    fn sketch(&self) -> ObjectId {
        match self {
            Self::Circle(r) => r.sketch,
            Self::Annulus(r) => r.sketch,
        }
    }
    fn expected(&self) -> ferritecad_document::DocumentVersion {
        match self {
            Self::Circle(r) => r.expected,
            Self::Annulus(r) => r.expected,
        }
    }
    fn at(&self, expected: ferritecad_document::DocumentVersion) -> Self {
        match self {
            Self::Circle(r) => Self::Circle(ferritecad_jobs::EditCircleRequest {
                expected,
                ..r.clone()
            }),
            Self::Annulus(r) => Self::Annulus(ferritecad_jobs::EditAnnulusRequest {
                expected,
                ..r.clone()
            }),
        }
    }
    /// The request the peer command line takes for the same numbers.
    fn peer(&self, source: &Path, root: &Path, out: &Path) {
        let (operation, json) = match self {
            Self::Circle(r) => (
                "edit-circle",
                format!(
                    r#"{{"request_version":1,"curve_id":"{}","center_mm":[{},{}],"radius_mm":{}}}"#,
                    r.edit.curve_id, r.edit.center_mm[0], r.edit.center_mm[1], r.edit.radius_mm
                ),
            ),
            Self::Annulus(r) => (
                "edit-annular",
                format!(
                    r#"{{"request_version":1,"outer_curve_id":"{}","inner_curve_id":"{}","center_mm":[{},{}],"outer_radius_mm":{},"inner_radius_mm":{}}}"#,
                    r.edit.outer_curve_id,
                    r.edit.inner_curve_id,
                    r.edit.center_mm[0],
                    r.edit.center_mm[1],
                    r.edit.outer_radius_mm,
                    r.edit.inner_radius_mm
                ),
            ),
        };
        let file = root.join("analytic-request.json");
        std::fs::write(&file, json).expect("request");
        let version = ferritecad_jobs::read_extrude_source(source)
            .expect("reading")
            .version;
        cli(&[
            operation.as_ref(),
            source.as_os_str(),
            "--sketch".as_ref(),
            self.sketch().to_string().as_ref(),
            "--expect-version".as_ref(),
            version.content.to_string().as_ref(),
            "--request".as_ref(),
            file.as_os_str(),
            "-o".as_ref(),
            out.as_os_str(),
        ]);
    }
}

/// Starts the worker the window starts for `request`.
fn start(
    sessions: &mut Sessions,
    request: &Request,
) -> (u64, mpsc::Receiver<Result<ferritecad_jobs::ProducedStep>>) {
    let (tx, rx) = mpsc::channel();
    let generation = sessions
        .begin_apply(|ticket, _, cancel| match request {
            Request::Circle(r) => spawn_apply_circle(
                ticket,
                r.sketch,
                r.edit,
                r.expected,
                cancel.clone(),
                move |result| tx.send(result).expect("deliver"),
            ),
            Request::Annulus(r) => spawn_apply_annulus(
                ticket,
                r.sketch,
                r.edit,
                r.expected,
                cancel.clone(),
                move |result| tx.send(result).expect("deliver"),
            ),
        })
        .expect("started");
    (generation, rx)
}

/// The window's own Apply of the Circle or annulus: the edit, the picture of the
/// new version, then the version becoming current. Returns what the edit meant.
fn apply_analytic(sessions: &mut Sessions, request: &Request) -> Edited {
    let (generation, rx) = start(sessions, request);
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(120))
        .expect("edit");
    let edited = sessions.finish_apply(generation, result);
    let Edited::Show(path) = &edited else {
        return edited;
    };
    let (tx, rx) = mpsc::channel();
    let token = sessions.scene_token(generation).expect("token");
    let worker = spawn_scene(path.clone(), token, move |scene| {
        tx.send(scene).expect("deliver")
    });
    assert!(sessions.attach_scene(generation, worker));
    rx.recv_timeout(std::time::Duration::from_secs(120))
        .expect("scene")
        .expect("the picture of the new version");
    sessions.bind(Bind::Staged).expect("bind");
    assert!(sessions.finish_scene(generation, Ok(())));
    edited
}

/// A request the worker refuses: the session keeps its version, history and
/// checkpoint, and no file is left. Returns what the window says.
fn refused_analytic(sessions: &mut Sessions, request: &Request) -> String {
    let before = (sessions.dirty(), sessions.can_undo(), sessions.can_redo());
    let shown = sessions.export_path().expect("accepted");
    let files = private_files(sessions);
    let (generation, rx) = start(sessions, request);
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(120))
        .expect("edit");
    assert_eq!(sessions.finish_apply(generation, result), Edited::Failed);
    assert!(sessions.status.starts_with("Could not apply the change"));
    assert_eq!(
        (sessions.dirty(), sessions.can_undo(), sessions.can_redo()),
        before,
        "a refused edit moved the session"
    );
    assert_eq!(private_files(sessions), files, "a refused edit left a file");
    assert_eq!(sessions.export_path().expect("accepted"), shown);
    sessions.status.clone()
}

// ---- the independent look at the mesh ---------------------------------------

struct Mesh {
    triangles: Vec<[[f64; 3]; 3]>,
}

fn mesh(stl: &[u8]) -> Mesh {
    let count = u32::from_le_bytes(stl[80..84].try_into().expect("count")) as usize;
    assert_eq!(stl.len(), 84 + 50 * count, "a binary STL of its own length");
    let float = |at: usize| f64::from(f32::from_le_bytes(stl[at..at + 4].try_into().expect("f32")));
    let triangles = (0..count)
        .map(|i| {
            let base = 84 + 50 * i + 12;
            std::array::from_fn(|corner| {
                std::array::from_fn(|axis| float(base + 12 * corner + 4 * axis))
            })
        })
        .collect();
    Mesh { triangles }
}

impl Mesh {
    /// One closed, consistently oriented surface: every directed edge once, and its
    /// reverse beside it.
    fn assert_closed_and_oriented(&self) {
        let key = |p: [f64; 3]| p.map(|n| (n * 1e4).round() as i64);
        let mut directed = std::collections::BTreeMap::new();
        for [a, b, c] in &self.triangles {
            for (u, v) in [(a, b), (b, c), (c, a)] {
                *directed.entry((key(*u), key(*v))).or_insert(0_u32) += 1;
            }
        }
        assert!(
            directed.values().all(|n| *n == 1),
            "not one oriented surface"
        );
        assert!(
            directed
                .keys()
                .all(|(u, v)| directed.contains_key(&(*v, *u))),
            "the mesh is open"
        );
    }
    fn volume(&self) -> f64 {
        self.triangles
            .iter()
            .map(|[a, b, c]| {
                a[0] * (b[1] * c[2] - b[2] * c[1])
                    + a[1] * (b[2] * c[0] - b[0] * c[2])
                    + a[2] * (b[0] * c[1] - b[1] * c[0])
            })
            .sum::<f64>()
            / 6.0
    }
    fn vertices(&self) -> impl Iterator<Item = [f64; 3]> + '_ {
        self.triangles.iter().flatten().copied()
    }
}

/// A cylinder of `radius` about `center` and `height` tall, hollow to `bore` when
/// there is one: closed, outward, the bounds and a volume that only the inscribed
/// polygon of the tessellation can lower.
fn assert_round(stl: &[u8], center: [f64; 2], radius: f64, bore: Option<f64>, height: f64) {
    let m = mesh(stl);
    m.assert_closed_and_oriented();
    let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
    for p in m.vertices() {
        for axis in 0..3 {
            lo[axis] = lo[axis].min(p[axis]);
            hi[axis] = hi[axis].max(p[axis]);
        }
    }
    for (got, want) in [
        (lo[0], center[0] - radius),
        (hi[0], center[0] + radius),
        (lo[1], center[1] - radius),
        (hi[1], center[1] + radius),
        (lo[2], 0.0),
        (hi[2], height),
    ] {
        assert!((got - want).abs() < 2e-2, "bounds {got} against {want}");
    }
    let area = std::f64::consts::PI * (radius * radius - bore.map_or(0.0, |b| b * b));
    let volume = m.volume();
    assert!(volume > 0.0, "the surface faces inward");
    assert!(
        volume <= area * height + 1e-3 && volume > 0.97 * area * height,
        "volume {volume} against {}",
        area * height
    );
    // Every vertex is on the boundary or on the bore: nothing of a solid disc is left.
    let radial = |p: [f64; 3]| ((p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2)).sqrt();
    let mut on_bore = 0;
    for p in m.vertices() {
        let r = radial(p);
        if (r - radius).abs() < 1e-2 {
            continue;
        }
        let bore = bore.expect("a vertex inside a solid cylinder's wall");
        assert!((r - bore).abs() < 1e-2, "a vertex at radius {r}");
        on_bore += 1;
    }
    assert_eq!(
        bore.is_some(),
        on_bore > 0,
        "the cavity is there or it is not"
    );
}

// ---- the gate ----------------------------------------------------------------

/// The curves of the Sketch with their radii, in the order they are stored.
fn stored_curves(path: &Path) -> Vec<(ferritecad_types::StableEntityId, [f64; 2], f64)> {
    let document = Document::open_read_only(path).expect("document");
    let sketch = document
        .objects()
        .expect("objects")
        .into_iter()
        .find_map(|o| match o.payload {
            ferritecad_document::ObjectPayload::Sketch(s) => Some(s),
            _ => None,
        })
        .expect("Sketch");
    sketch
        .curves
        .iter()
        .map(|c| match c.geometry {
            ferritecad_document::SketchGeometry::Circle { center, radius } => {
                (c.id, [center.x, center.y], radius)
            }
            _ => panic!("a Circle profile stores only Circles"),
        })
        .collect()
}

/// Every table but the two that an edit is allowed to touch is the same as the
/// original's; the object rows keep their ids, parents, names and order.
fn assert_identity_kept(original: &Path, edited: &Path) {
    let (a, b) = (cells(original), cells(edited));
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    for (table, rows) in &a {
        if table == "objects" || table == "meta" {
            continue;
        }
        assert_eq!(
            rows, &b[table],
            "{table} changed: an identity or a name moved"
        );
    }
    let ids = |path: &Path| -> Vec<(ObjectId, Option<String>)> {
        Document::open_read_only(path)
            .expect("document")
            .objects()
            .expect("objects")
            .into_iter()
            .map(|o| (o.id, o.name))
            .collect()
    };
    assert_eq!(ids(original), ids(edited), "an object id or name moved");
}

struct Numbers {
    height: f64,
    after: f64,
}

/// Open, Apply the height, Apply the geometry through the widgets, export while
/// unsaved, Undo, Undo, Redo, Redo, Save and a cold reopen on `source`, each
/// against the command line doing the same to the same document; then refusals
/// between steps, a no-op, a stale form and a branch after Undo.
fn analytic_gate(
    name: &str,
    root: &Path,
    source: &Path,
    numbers: &Numbers,
    draft: impl FnOnce(
        &Path,
        &ferritecad_document::ExtrudeEditSource,
    ) -> (crate::sketch::Editor, Request),
    check_mesh: impl Fn(&[u8], bool),
) {
    let original = std::fs::read(source).expect("source");
    let opened = ferritecad_jobs::read_extrude_source(source).expect("reading");
    let feature = opened.features[0].feature;
    let private = tempfile::tempdir().expect("private root");
    let mut sessions = Sessions::default();
    sessions.adopt(
        DocumentSession::open_in(private.path(), source, HistoryLimits::default())
            .expect("session"),
    );
    let alias = sessions.logical_path().expect("logical").to_path_buf();

    apply_native(&mut sessions, feature, numbers.height);
    let after_height = sessions.export_path().expect("accepted");
    let reading = ferritecad_jobs::read_extrude_source(&after_height).expect("reading");
    let (form, request) = draft(&after_height, &reading);
    assert_eq!(request.expected(), reading.version, "the form's version");

    // The command line, doing the same two things to the same document.
    let peer1 = root.join("peer-height.fcad");
    let peer2 = root.join("peer-geometry.fcad");
    cli(&[
        "edit-extrude".as_ref(),
        source.as_os_str(),
        "--feature".as_ref(),
        feature.to_string().as_ref(),
        "--expect-version".as_ref(),
        opened.version.content.to_string().as_ref(),
        "--distance-mm".as_ref(),
        numbers.height.to_string().as_ref(),
        "-o".as_ref(),
        peer1.as_os_str(),
    ]);
    request.peer(&peer1, root, &peer2);
    let (original_stl, original_fbx) = peer_bytes(source, root, "a-original");
    let (h_stl, h_fbx) = peer_bytes(&peer1, root, "a-height");
    let (g_stl, g_fbx) = peer_bytes(&peer2, root, "a-geometry");
    assert_ne!(h_stl, g_stl, "the geometry edit must change the part");

    // Apply: accepted into the session, the file on disk untouched, the form over.
    assert!(matches!(
        apply_analytic(&mut sessions, &request),
        Edited::Show(_)
    ));
    let mut form = form;
    assert!(form.editing_analytic());
    form.finish_session_change();
    assert!(!form.active(), "the form outlived the picture it described");
    assert!(sessions.dirty());
    assert_eq!(
        std::fs::read(source).expect("source"),
        original,
        "Apply wrote the file"
    );
    let applied = sessions.export_path().expect("accepted");
    assert_eq!(
        cells(&applied),
        cells(&peer2),
        "the saved model is not the command line's, in some cell"
    );
    assert_identity_kept(&after_height, &applied);

    // Export while unsaved is the accepted model, not the file on disk.
    let (stl, fbx) = export_bytes(&applied, &alias, root, "a-unsaved");
    assert_eq!(
        (stl.clone(), fbx),
        (g_stl.clone(), g_fbx.clone()),
        "unsaved export"
    );
    assert_ne!(stl, original_stl, "the export read the file on disk");
    check_mesh(&stl, true);

    // Undo is the height step, Undo again the file; Redo comes back identically.
    move_native(&mut sessions, true);
    let (stl, fbx) = export_bytes(
        &sessions.export_path().expect("accepted"),
        &alias,
        root,
        "a-undo1",
    );
    assert_eq!((stl, fbx), (h_stl, h_fbx), "Undo: height");
    move_native(&mut sessions, true);
    assert!(!sessions.dirty());
    let (stl, fbx) = export_bytes(
        &sessions.export_path().expect("accepted"),
        &alias,
        root,
        "a-undo2",
    );
    assert_eq!(
        (stl, fbx),
        (original_stl, original_fbx),
        "Undo did not restore"
    );
    move_native(&mut sessions, false);
    move_native(&mut sessions, false);
    assert!(sessions.dirty() && !sessions.can_redo());
    assert_eq!(
        cells(&sessions.export_path().expect("accepted")),
        cells(&peer2)
    );
    assert_eq!(std::fs::read(source).expect("source"), original);

    // Save is the command line's second copy, cell for cell, and every saved name
    // resolves on a cold rebuild of the file.
    let (tx, rx) = mpsc::channel();
    let generation = sessions
        .begin_save(SaveTarget::InPlace, None, |plan, _, cancel| {
            spawn_save(plan, cancel.clone(), move |result| {
                tx.send(result).expect("deliver")
            })
        })
        .expect("started");
    let report = sessions
        .finish_save(generation, rx.recv().expect("answer"))
        .expect("answered");
    assert!(report.published && !sessions.dirty());
    assert_eq!(
        cells(source),
        cells(&peer2),
        "Save is not the command line's copy"
    );
    let saved = Document::open_read_only(source).expect("saved");
    let built = ferritecad_eval::rebuild_cold(
        &saved,
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &OperationContext::default(),
    )
    .expect("cold rebuild");
    let references = saved.topology_refs().expect("refs");
    assert!(!references.is_empty(), "the part carries saved names");
    for reference in references {
        assert!(
            built
                .resolve(&reference)
                .is_ok_and(|found| !found.is_empty()),
            "{} did not resolve after Save",
            reference.id
        );
    }
    drop(saved);
    let (stl, fbx) = peer_bytes(source, root, "a-saved");
    assert_eq!((stl.clone(), fbx.clone()), (g_stl, g_fbx));
    check_mesh(&stl, true);
    // The saved model's exports, for the pinned independent reader the workflow
    // runs over them (`tools/check-fbx-complex.sh`).
    if let Some(directory) = std::env::var_os("FCAD_ANALYTIC_ARTIFACTS") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).expect("artifact directory");
        std::fs::write(directory.join(format!("{name}.stl")), &stl).expect("stl");
        std::fs::write(directory.join(format!("{name}.fbx")), &fbx).expect("fbx");
    }

    // From the saved model: Undo, then everything that must change nothing.
    move_native(&mut sessions, true);
    assert!(sessions.can_redo());
    let current = ferritecad_jobs::read_extrude_source(&sessions.export_path().expect("accepted"))
        .expect("reading")
        .version;

    // A form made from an older version is refused, and so is a request the
    // document will not store; neither cuts off the Redo.
    let stale = refused_analytic(&mut sessions, &request.at(opened.version));
    assert!(
        stale.contains("document changed after this form was opened"),
        "{stale}"
    );
    assert!(sessions.can_redo(), "a refusal cut off the Redo");
    let forged = match request.at(current) {
        Request::Circle(mut r) => {
            r.edit.radius_mm = 0.0;
            Request::Circle(r)
        }
        Request::Annulus(mut r) => {
            r.edit.inner_radius_mm = r.edit.outer_radius_mm;
            Request::Annulus(r)
        }
    };
    refused_analytic(&mut sessions, &forged);
    assert!(sessions.can_redo(), "a refusal cut off the Redo");

    // The numbers that are already stored are no step: nothing is recorded and
    // the Redo survives.
    let stored = {
        let curves = stored_curves(&sessions.export_path().expect("accepted"));
        match request.at(current) {
            Request::Circle(mut r) => {
                r.edit.center_mm = curves[0].1;
                r.edit.radius_mm = curves[0].2;
                Request::Circle(r)
            }
            Request::Annulus(mut r) => {
                let (outer, inner) = if curves[0].2 > curves[1].2 {
                    (curves[0], curves[1])
                } else {
                    (curves[1], curves[0])
                };
                r.edit.center_mm = outer.1;
                r.edit.outer_radius_mm = outer.2;
                r.edit.inner_radius_mm = inner.2;
                Request::Annulus(r)
            }
        }
    };
    let before = (sessions.dirty(), sessions.can_undo(), sessions.can_redo());
    let files = private_files(&sessions);
    assert_eq!(apply_analytic(&mut sessions, &stored), Edited::NoChange);
    assert!(
        sessions.status.starts_with("No change"),
        "{}",
        sessions.status
    );
    assert_eq!(
        (sessions.dirty(), sessions.can_undo(), sessions.can_redo()),
        before
    );
    assert_eq!(private_files(&sessions), files, "a no-op left a file");

    // A branch that succeeds cuts the Redo; the file is still the saved copy.
    apply_native(&mut sessions, feature, numbers.after);
    assert!(!sessions.can_redo());
    assert_eq!(cells(source), cells(&peer2), "the branch wrote the file");
}

fn create(root: &Path, operation: &str, json: &str) -> PathBuf {
    let source = root.join(format!("{operation}.fcad"));
    let input = root.join(format!("{operation}.json"));
    std::fs::write(&input, json).expect("request");
    cli(&[
        operation.as_ref(),
        input.as_os_str(),
        "-o".as_ref(),
        source.as_os_str(),
    ]);
    source
}

#[test]
fn native_a_circle_with_a_fractional_offset_is_applied_like_the_command_line() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = create(
        root.path(),
        "create-circle-extrude",
        r#"{"schema_version":1,"center_mm":[12.5,-7.25],"radius_mm":10.5,"height_mm":15.25}"#,
    );
    let sketch = ferritecad_jobs::read_extrude_source(&source)
        .expect("reading")
        .circle_sketches[0]
        .sketch;
    analytic_gate(
        "circle-session",
        root.path(),
        &source,
        &Numbers {
            height: 8.5,
            after: 11.25,
        },
        |path, reading| {
            let (form, request) = typed_circle(path, reading, sketch, ["-3.5", "4.25"], "6.75");
            (form, Request::Circle(request))
        },
        |stl, applied| {
            if applied {
                assert_round(stl, [-3.5, 4.25], 6.75, None, 8.5);
            }
        },
    );
    // The same circle keeps its own UUIDs: the Sketch, the curve, the feature.
    let curves = stored_curves(&source);
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].1, [-3.5, 4.25]);
    assert_eq!(curves[0].2, 6.75);
}

#[test]
fn native_an_annulus_with_unequal_fractional_radii_is_applied_like_the_command_line() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = create(
        root.path(),
        "create-annular-extrude",
        r#"{"schema_version":1,"center_mm":[12.5,-7.25],"outer_radius_mm":10.5,"inner_radius_mm":4.75,"height_mm":15.25}"#,
    );
    annulus_gate("annulus-session", root.path(), &source);
}

#[test]
fn native_an_annulus_stored_bore_first_keeps_its_roles_by_curve_uuid() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("swapped.fcad");
    write_round(&source, [12.5, -7.25], &[10.5, 4.75], 15.25, true);
    let stored = stored_curves(&source);
    assert!(stored[0].2 < stored[1].2, "the bore is stored first");
    annulus_gate("annulus-swapped-session", root.path(), &source);
    // After Save the order is still the stored one, and each UUID still holds its
    // role: the boundary UUID the larger radius, the bore UUID the smaller.
    let after = stored_curves(&source);
    assert_eq!(
        after.iter().map(|c| c.0).collect::<Vec<_>>(),
        stored.iter().map(|c| c.0).collect::<Vec<_>>(),
        "the stored order or a UUID changed"
    );
    assert_eq!(after[0].2, 3.125, "the bore UUID holds the new bore radius");
    assert_eq!(after[1].2, 9.25, "the boundary UUID holds the new radius");
    assert!(after.iter().all(|c| c.1 == [-3.5, 4.25]));
}

fn annulus_gate(name: &str, root: &Path, source: &Path) {
    let sketch = ferritecad_jobs::read_extrude_source(source)
        .expect("reading")
        .annulus_sketches[0]
        .sketch;
    analytic_gate(
        name,
        root,
        source,
        &Numbers {
            height: 8.5,
            after: 11.25,
        },
        |path, reading| {
            let (form, request) =
                typed_annulus(path, reading, sketch, ["-3.5", "4.25"], "9.25", "3.125");
            (form, Request::Annulus(request))
        },
        |stl, applied| {
            if applied {
                assert_round(stl, [-3.5, 4.25], 9.25, Some(3.125), 8.5);
            }
        },
    );
}

/// Build without a kernel: the workers are refused whole, nothing becomes dirty,
/// and no file is touched. (The geometry edits do not ask for the sketch solver.)
#[test]
fn without_a_kernel_a_circle_apply_is_refused_and_the_document_stays_clean() {
    if ferritecad_occt::is_available() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("circle.fcad");
    write_round(&source, [12.5, -7.25], &[10.5], 15.25, false);
    let reading = ferritecad_jobs::read_extrude_source(&source).expect("reading");
    let sketch = reading.circle_sketches[0].sketch;
    let (_form, request) = typed_circle(&source, &reading, sketch, ["1", "2"], "3");
    let original = std::fs::read(&source).expect("bytes");
    let private = tempfile::tempdir().expect("private");
    let mut sessions = Sessions::default();
    sessions.adopt(
        DocumentSession::open_in(private.path(), &source, HistoryLimits::default())
            .expect("session"),
    );
    let files = private_files(&sessions);
    let (generation, rx) = start(&mut sessions, &Request::Circle(request));
    let result = rx.recv().expect("answer");
    assert_eq!(sessions.finish_apply(generation, result), Edited::Failed);
    assert!(
        sessions.status.starts_with("Could not apply the change"),
        "{}",
        sessions.status
    );
    assert!(!sessions.dirty() && !sessions.can_undo());
    assert_eq!(private_files(&sessions), files);
    assert_eq!(std::fs::read(&source).expect("bytes"), original);
}

/// The geometry editors are for unconstrained profiles. A constraint applied
/// through the session withdraws the Circle editor from the next catalogue, and
/// removing it through the session offers the editor again: availability follows
/// the accepted version, not the form that was open.
#[test]
fn native_constraints_applied_through_the_session_withdraw_and_restore_the_circle_editor() {
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
    let root = tempfile::tempdir().expect("root");
    let source = create(
        root.path(),
        "create-circle-extrude",
        r#"{"schema_version":1,"center_mm":[12.5,-7.25],"radius_mm":10.5,"height_mm":15.25}"#,
    );
    let opened = ferritecad_jobs::read_extrude_source(&source).expect("reading");
    let sketch = opened.constraint_sketches[0].sketch;
    assert!(opened.circle_sketches[0].refusal.is_none());
    let private = tempfile::tempdir().expect("private root");
    let mut sessions = Sessions::default();
    sessions.adopt(
        DocumentSession::open_in(private.path(), &source, HistoryLimits::default())
            .expect("session"),
    );
    let (_form, constraints) = crate::constraints::tests::session_apply::typed_circle(
        &source,
        &opened,
        sketch,
        "6.75",
        ("-3.5", "4.25"),
    );
    apply_constraints_native(&mut sessions, &constraints);
    let constrained =
        ferritecad_jobs::read_extrude_source(&sessions.export_path().expect("accepted"))
            .expect("reading");
    let refusal = constrained.circle_sketches[0]
        .refusal
        .as_deref()
        .expect("the Circle editor is withdrawn once the profile is constrained");
    assert!(refusal.contains("constrain"), "{refusal}");
    let mut form = crate::sketch::Editor::default();
    assert!(
        !form.begin_circle_edit(&source, &constrained, constrained.circle_sketches[0].sketch),
        "the withdrawn editor opened"
    );

    // Removing both constraints through the session offers it again.
    let ids: Vec<_> = stored_constraints(&sessions.export_path().expect("accepted"))
        .iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(ids.len(), 2);
    apply_constraints_native(
        &mut sessions,
        &ferritecad_jobs::EditSketchConstraintsRequest {
            source: source.clone(),
            expected: constrained.version,
            sketch,
            edits: ferritecad_document::SketchConstraintEdits {
                remove: ids,
                add: Vec::new(),
            },
            destination: PathBuf::new(),
        },
    );
    let restored = ferritecad_jobs::read_extrude_source(&sessions.export_path().expect("accepted"))
        .expect("reading");
    assert!(restored.circle_sketches[0].refusal.is_none());
    assert!(form.begin_circle_edit(&source, &restored, restored.circle_sketches[0].sketch));
}
