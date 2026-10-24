// SPDX-License-Identifier: MIT
//! §30D: a saved Circle or annulus applied inside the open document, on the real
//! workers and against the real peer command line. Nothing here introduces a new
//! identity, so the saved document is compared with the command line's copy in
//! every SQL cell: no Sketch row and no payload is set aside.

use super::*;
use crate::sketch::tests::analytic_apply::{typed_annulus, typed_circle, write_round};

enum Request {
    Circle(ferritecad_jobs::EditRequest),
    Annulus(ferritecad_jobs::EditRequest),
}

impl Request {
    fn expected(&self) -> ferritecad_document::DocumentVersion {
        match self {
            Self::Circle(r) => r.expected,
            Self::Annulus(r) => r.expected,
        }
    }
    fn at(&self, expected: ferritecad_document::DocumentVersion) -> Self {
        match self {
            Self::Circle(r) => Self::Circle(ferritecad_jobs::EditRequest {
                expected,
                ..r.clone()
            }),
            Self::Annulus(r) => Self::Annulus(ferritecad_jobs::EditRequest {
                expected,
                ..r.clone()
            }),
        }
    }
    /// The request the peer command line takes for the same numbers.
    fn peer(&self, source: &Path, root: &Path, out: &Path) {
        let changes = match self {
            Self::Circle(r) | Self::Annulus(r) => &r.changes,
        };
        let json = ferritecad_fixtures::edit_request_json(changes).to_string();
        let file = root.join("analytic-request.json");
        std::fs::write(&file, json).expect("request");
        let version = crate::sketch_facts::read(source).expect("reading").version;
        cli(&[
            "edit".as_ref(),
            source.as_os_str(),
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
) -> (
    Address,
    mpsc::Receiver<Result<ferritecad_jobs::ProducedStep>>,
) {
    let (tx, rx) = mpsc::channel();
    let generation = sessions
        .begin_apply(|ticket, _, cancel| match request {
            Request::Circle(r) => spawn_apply(
                r.expected,
                ticket,
                r.changes.clone(),
                cancel.clone(),
                move |result| tx.send(result).expect("deliver"),
            ),
            Request::Annulus(r) => spawn_apply(
                r.expected,
                ticket,
                r.changes.clone(),
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
pub(super) fn assert_round(
    stl: &[u8],
    center: [f64; 2],
    radius: f64,
    bore: Option<f64>,
    height: f64,
) {
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
    draft: impl FnOnce(&Path, &crate::sketch_facts::Source) -> (crate::sketch::Editor, Request),
    check_mesh: impl Fn(&[u8], bool),
) {
    let original = std::fs::read(source).expect("source");
    let opened = crate::sketch_facts::read(source).expect("reading");
    let feature = ferritecad_fixtures::expression_slot(
        &opened,
        ferritecad_document::ExpressionSlot::ExtrudeDistance,
    )
    .object;
    let private = tempfile::tempdir().expect("private root");
    let mut sessions = Sessions::default();
    sessions.adopt(
        crate::sessions::test_open::open_in(private.path(), source, HistoryLimits::default())
            .expect("session"),
    );
    let alias = sessions.logical_path().expect("logical").to_path_buf();

    apply_native(&mut sessions, feature, numbers.height);
    let after_height = sessions.export_path().expect("accepted");
    let reading = crate::sketch_facts::read(&after_height).expect("reading");
    let (form, request) = draft(&after_height, &reading);
    assert_eq!(request.expected(), reading.version, "the form's version");

    // The command line, doing the same two things to the same document.
    let peer1 = root.join("peer-height.fcad");
    let peer2 = root.join("peer-geometry.fcad");
    {
        let request = crate::edits::tests::expression_request(
            &feature.to_string(),
            "extrude_distance",
            &numbers.height.to_string(),
            "mm",
        );
        cli(&[
            "edit".as_ref(),
            source.as_os_str(),
            "--expect-version".as_ref(),
            opened.version.content.to_string().as_ref(),
            "--request".as_ref(),
            request.path().as_os_str(),
            "--output".as_ref(),
            peer1.as_os_str(),
        ])
    };
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
    let current = crate::sketch_facts::read(&sessions.export_path().expect("accepted"))
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
            let v = crate::transaction_tests::circle(&r);
            crate::transaction_tests::set_circle(&mut r, 0, v.center_mm, 0.);
            Request::Circle(r)
        }
        Request::Annulus(mut r) => {
            let v = crate::transaction_tests::annulus(&r);
            crate::transaction_tests::set_circle(&mut r, 1, v.center_mm, v.outer_radius_mm);
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
                crate::transaction_tests::set_circle(&mut r, 0, curves[0].1, curves[0].2);
                Request::Circle(r)
            }
            Request::Annulus(mut r) => {
                let (outer, inner) = if curves[0].2 > curves[1].2 {
                    (curves[0], curves[1])
                } else {
                    (curves[1], curves[0])
                };
                crate::transaction_tests::set_circle(&mut r, 0, outer.1, outer.2);
                crate::transaction_tests::set_circle(&mut r, 1, inner.1, inner.2);
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

pub(super) fn create(root: &Path, operation: &str, json: &str) -> PathBuf {
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
    let sketch = crate::sketch_facts::read(&source)
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
    let sketch = crate::sketch_facts::read(source)
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

// ---- §31I: the canvas of the forms, through the same route -------------------

/// The walls of a cold rebuild of `path`, each under its own saved reference: the
/// curve UUID the reference was raised from and the surface the kernel reports.
fn walls_by_reference(
    path: &Path,
) -> std::collections::BTreeMap<ferritecad_types::StableEntityId, ferritecad_kernel::FaceSurface> {
    let document = Document::open_read_only(path).expect("reopen");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built = ferritecad_eval::rebuild_cold(&document, &mut kernel, &OperationContext::default())
        .expect("cold rebuild");
    let mut walls = std::collections::BTreeMap::new();
    let references = document.topology_refs().expect("refs");
    assert!(!references.is_empty(), "the part carries saved names");
    for reference in &references {
        let handles = built.resolve(reference).expect("resolve");
        assert_eq!(handles.len(), 1, "{:?}", reference.output_role);
        if let ferritecad_document::SemanticRole::ExtrudeSide { profile_segment } =
            reference.output_role
        {
            walls.insert(
                profile_segment,
                kernel.face_surface(handles[0]).expect("surface"),
            );
        }
    }
    built.release_all(&mut kernel);
    document.close().expect("close");
    walls
}

/// §31I: the form's canvas dragged by real events (`draft`), the window's own Apply on
/// the real worker, an export before Save, Undo, Redo, Save and a cold reopen of
/// `source`, each against the command line given the numbers the canvas wrote. The
/// height, every identity and reference and the source file (until Save) are kept.
fn canvas_gate(
    name: &str,
    root: &Path,
    source: &Path,
    draft: impl FnOnce(&Path, &crate::sketch_facts::Source) -> (crate::sketch::Editor, Request),
) -> (Request, Vec<u8>) {
    let original = std::fs::read(source).expect("source");
    let private = tempfile::tempdir().expect("private root");
    let mut sessions = Sessions::default();
    sessions.adopt(
        crate::sessions::test_open::open_in(private.path(), source, HistoryLimits::default())
            .expect("session"),
    );
    let alias = sessions.logical_path().expect("logical").to_path_buf();
    let opened = sessions.export_path().expect("accepted");
    let reading = crate::sketch_facts::read(&opened).expect("reading");
    let (mut form, request) = draft(&opened, &reading);
    assert_eq!(request.expected(), reading.version, "the form's version");

    // The command line, given the same explicit numbers.
    let peer = root.join(format!("{name}-peer.fcad"));
    request.peer(source, root, &peer);
    let (original_stl, original_fbx) = peer_bytes(source, root, &format!("{name}-original"));
    let (g_stl, g_fbx) = peer_bytes(&peer, root, &format!("{name}-geometry"));
    assert_ne!(g_stl, original_stl, "the gestures must change the part");

    // Apply: one session step, the form over, the file untouched.
    assert!(matches!(
        apply_analytic(&mut sessions, &request),
        Edited::Show(_)
    ));
    form.finish_session_change();
    assert!(!form.active(), "the form outlived the picture it described");
    assert_eq!(
        std::fs::read(source).expect("source"),
        original,
        "Apply wrote the file"
    );
    let applied = sessions.export_path().expect("accepted");
    assert_eq!(
        cells(&applied),
        cells(&peer),
        "not the command line's model"
    );
    assert_identity_kept(source, &applied);

    // Export before Save is the accepted model; Undo and Redo move it exactly.
    let (stl, fbx) = export_bytes(&applied, &alias, root, &format!("{name}-unsaved"));
    assert!(stl == g_stl && fbx == g_fbx, "unsaved export");
    move_native(&mut sessions, true);
    assert!(!sessions.dirty());
    let (stl, fbx) = export_bytes(
        &sessions.export_path().expect("accepted"),
        &alias,
        root,
        &format!("{name}-undo"),
    );
    assert!(stl == original_stl && fbx == original_fbx, "Undo");
    move_native(&mut sessions, false);
    assert!(sessions.dirty() && !sessions.can_redo());
    assert_eq!(
        cells(&sessions.export_path().expect("accepted")),
        cells(&peer)
    );
    assert_eq!(std::fs::read(source).expect("source"), original);

    // Save is the command line's copy in every cell; a cold reopen resolves it.
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
        cells(&peer),
        "Save is not the command line's copy"
    );
    let (stl, fbx) = peer_bytes(source, root, &format!("{name}-saved"));
    assert!(stl == g_stl && fbx == g_fbx, "the saved file's exports");
    if let Some(directory) = std::env::var_os("FCAD_ANALYTIC_ARTIFACTS") {
        let directory = PathBuf::from(directory);
        std::fs::create_dir_all(&directory).expect("artifact directory");
        std::fs::write(directory.join(format!("{name}.stl")), &stl).expect("stl");
        std::fs::write(directory.join(format!("{name}.fbx")), &fbx).expect("fbx");
    }
    (request, stl)
}

/// A height of the saved Extrude, read back.
fn saved_height(path: &Path) -> f64 {
    let reading = crate::sketch_facts::read(path).expect("reading");
    reading
        .circle_sketches
        .iter()
        .filter_map(|c| c.circle.as_ref().and_then(|c| c.height_mm))
        .chain(
            reading
                .annulus_sketches
                .iter()
                .filter_map(|a| a.annulus.as_ref().and_then(|a| a.height_mm)),
        )
        .next()
        .expect("one round profile")
}

/// §31I/§31J: what a saved Circle is after the canvas's numbers were applied and saved:
/// an offset, fractional centre and radius, the same curve UUID and height, the wall
/// under the saved reference a cylinder of that radius, and a mesh of that cylinder.
fn assert_circle_canvas_result(
    name: &str,
    source: &Path,
    before: &[(ferritecad_types::StableEntityId, [f64; 2], f64)],
    request: Request,
    stl: &[u8],
) {
    let Request::Circle(request) = request else {
        panic!("a circle request");
    };
    let curves = requested_circles(&request, before);
    let (centre, radius) = (curves[0].1, curves[0].2);
    eprintln!("{name} centre={centre:?} radius={radius} height=15.25");
    // An offset, fractional centre and radius, as the pointer made them.
    assert!(centre[0] != 12.5 && centre[1] != -7.25 && centre.iter().all(|c| c.fract() != 0.));
    assert!(radius != 10.5 && radius.fract() != 0.);
    let after = stored_curves(source);
    assert_eq!(
        (after.len(), after[0].0),
        (1, before[0].0),
        "the curve UUID changed"
    );
    assert_eq!((after[0].1, after[0].2), (centre, radius));
    assert_eq!(saved_height(source), 15.25, "the height moved");
    let walls = walls_by_reference(source);
    let ferritecad_kernel::FaceSurface::Cylinder { radius: wall } = walls[&before[0].0] else {
        panic!("the wall is not a cylinder: {walls:?}");
    };
    assert!((wall - radius).abs() < 1e-9, "{wall} against {radius}");
    assert_round(stl, centre, radius, None, 15.25);
}

/// §31I/§31J: the same for a saved annulus: each curve UUID keeps its role and holds its
/// own new radius about the new centre, each wall under its saved reference.
fn assert_annulus_canvas_result(
    name: &str,
    source: &Path,
    before: &[(ferritecad_types::StableEntityId, [f64; 2], f64)],
    request: Request,
    stl: &[u8],
) {
    let Request::Annulus(request) = request else {
        panic!("an annulus request");
    };
    let mut curves = requested_circles(&request, before);
    curves.sort_by(|a, b| b.2.total_cmp(&a.2));
    let edit = crate::transaction_tests::AnnulusValues {
        outer_curve_id: curves[0].0,
        inner_curve_id: curves[1].0,
        center_mm: curves[0].1,
        outer_radius_mm: curves[0].2,
        inner_radius_mm: curves[1].2,
    };
    eprintln!(
        "{name} centre={:?} outer={} bore={} height=15.25",
        edit.center_mm, edit.outer_radius_mm, edit.inner_radius_mm
    );
    assert!(edit.center_mm.iter().all(|c| c.fract() != 0.));
    assert!(edit.outer_radius_mm != 10.5 && edit.inner_radius_mm != 4.75);
    assert!(edit.outer_radius_mm.fract() != 0. && edit.inner_radius_mm.fract() != 0.);
    assert!(edit.outer_radius_mm > edit.inner_radius_mm + 1.);
    // Each UUID keeps its role and holds its own new radius about the new centre.
    let after = stored_curves(source);
    let ids = |curves: &[(ferritecad_types::StableEntityId, [f64; 2], f64)]| {
        curves.iter().map(|c| c.0).collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&after),
        ids(before),
        "a UUID or the stored order changed"
    );
    let radius_of = |id| after.iter().find(|c| c.0 == id).expect("curve").2;
    assert_eq!(radius_of(edit.outer_curve_id), edit.outer_radius_mm);
    assert_eq!(radius_of(edit.inner_curve_id), edit.inner_radius_mm);
    assert!(after.iter().all(|c| c.1 == edit.center_mm));
    assert_eq!(saved_height(source), 15.25, "the height moved");
    // The surface under the outer wall's saved reference and under the bore's.
    let walls = walls_by_reference(source);
    assert_eq!(walls.len(), 2, "{walls:?}");
    for (id, want) in [
        (edit.outer_curve_id, edit.outer_radius_mm),
        (edit.inner_curve_id, edit.inner_radius_mm),
    ] {
        let ferritecad_kernel::FaceSurface::Cylinder { radius } = walls[&id] else {
            panic!("the wall of {id} is not a cylinder: {walls:?}");
        };
        assert!(
            (radius - want).abs() < 1e-9,
            "{id}: {radius} against {want}"
        );
    }
    assert_round(
        stl,
        edit.center_mm,
        edit.outer_radius_mm,
        Some(edit.inner_radius_mm),
        15.25,
    );
}

#[test]
fn native_a_circle_moved_and_resized_on_its_canvas_is_applied_and_saved_like_the_command_line() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = create(
        root.path(),
        "create-circle-extrude",
        r#"{"schema_version":1,"center_mm":[12.5,-7.25],"radius_mm":10.5,"height_mm":15.25}"#,
    );
    let before = stored_curves(&source);
    let (request, stl) = canvas_gate("circle-canvas", root.path(), &source, |path, reading| {
        let sketch = reading.circle_sketches[0].sketch;
        let (form, request) =
            crate::sketch::analytic_tests::dragged_circle_apply(path, reading, sketch);
        (form, Request::Circle(request))
    });
    assert_circle_canvas_result("circle-canvas", &source, &before, request, &stl);
}

#[test]
fn native_an_annulus_moved_and_resized_on_its_canvas_keeps_each_wall_under_its_reference() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = create(
        root.path(),
        "create-annular-extrude",
        r#"{"schema_version":1,"center_mm":[12.5,-7.25],"outer_radius_mm":10.5,"inner_radius_mm":4.75,"height_mm":15.25}"#,
    );
    let before = stored_curves(&source);
    let (request, stl) = canvas_gate("annulus-canvas", root.path(), &source, |path, reading| {
        let sketch = reading.annulus_sketches[0].sketch;
        let (form, request) =
            crate::sketch::analytic_tests::dragged_annulus_apply(path, reading, sketch);
        (form, Request::Annulus(request))
    });
    assert_annulus_canvas_result("annulus-canvas", &source, &before, request, &stl);
}

/// §31J: what navigating the canvas alone leaves alone, checked once the view has moved
/// and before anything is dragged: the session's file and the source file (cells and bytes),
/// the command line's exports of the source, and the version the form was read from.
struct Untouched {
    cells: std::collections::BTreeMap<String, Vec<String>>,
    bytes: Vec<u8>,
    exports: (Vec<u8>, Vec<u8>),
    version: ferritecad_document::DocumentVersion,
}

impl Untouched {
    fn before(
        source: &Path,
        root: &Path,
        name: &str,
        version: ferritecad_document::DocumentVersion,
    ) -> Self {
        Self {
            cells: cells(source),
            bytes: std::fs::read(source).expect("source"),
            exports: peer_bytes(source, root, &format!("{name}-before-navigation")),
            version,
        }
    }
    fn after(&self, source: &Path, session_copy: &Path, root: &Path, name: &str) {
        assert_eq!(
            cells(session_copy),
            self.cells,
            "navigation touched the session's file"
        );
        assert_eq!(
            cells(source),
            self.cells,
            "navigation touched the source's cells"
        );
        assert_eq!(
            std::fs::read(source).expect("source"),
            self.bytes,
            "the source file"
        );
        let now = peer_bytes(source, root, &format!("{name}-after-navigation"));
        assert!(
            now.0 == self.exports.0 && now.1 == self.exports.1,
            "the source's STL or FBX moved"
        );
        assert_eq!(
            crate::sketch_facts::read(session_copy)
                .expect("reading")
                .version,
            self.version,
            "the version the form was read from moved"
        );
    }
}

/// §31J: the §31I route with the canvas zoomed and panned first (and Fit pressed once): that
/// alone leaves every SQL cell, the file and its exports as they were; the drag is then
/// made at the new scale, applied on the real worker, exported before Save, undone,
/// redone and saved, each against the command line given the computed numbers.
#[test]
fn native_a_circle_dragged_at_a_navigated_scale_is_applied_and_saved_like_the_command_line() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = create(
        root.path(),
        "create-circle-extrude",
        r#"{"schema_version":1,"center_mm":[12.5,-7.25],"radius_mm":10.5,"height_mm":15.25}"#,
    );
    let before = stored_curves(&source);
    let (request, stl) = canvas_gate("circle-navigated", root.path(), &source, |path, reading| {
        let sketch = reading.circle_sketches[0].sketch;
        let untouched =
            Untouched::before(&source, root.path(), "circle-navigated", reading.version);
        let (form, request) =
            crate::sketch::navigation_tests::navigated_circle_apply(path, reading, sketch, || {
                untouched.after(&source, path, root.path(), "circle-navigated")
            });
        (form, Request::Circle(request))
    });
    assert_circle_canvas_result("circle-navigated", &source, &before, request, &stl);
}

#[test]
fn native_an_annulus_dragged_at_a_navigated_scale_keeps_each_wall_under_its_reference() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = create(
        root.path(),
        "create-annular-extrude",
        r#"{"schema_version":1,"center_mm":[12.5,-7.25],"outer_radius_mm":10.5,"inner_radius_mm":4.75,"height_mm":15.25}"#,
    );
    let before = stored_curves(&source);
    let (request, stl) = canvas_gate(
        "annulus-navigated",
        root.path(),
        &source,
        |path, reading| {
            let sketch = reading.annulus_sketches[0].sketch;
            let untouched =
                Untouched::before(&source, root.path(), "annulus-navigated", reading.version);
            let (form, request) = crate::sketch::navigation_tests::navigated_annulus_apply(
                path,
                reading,
                sketch,
                || untouched.after(&source, path, root.path(), "annulus-navigated"),
            );
            (form, Request::Annulus(request))
        },
    );
    assert_annulus_canvas_result("annulus-navigated", &source, &before, request, &stl);
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
    let reading = crate::sketch_facts::read(&source).expect("reading");
    let sketch = reading.circle_sketches[0].sketch;
    let (_form, request) = typed_circle(&source, &reading, sketch, ["1", "2"], "3");
    let original = std::fs::read(&source).expect("bytes");
    let private = tempfile::tempdir().expect("private");
    let mut sessions = Sessions::default();
    sessions.adopt(
        crate::sessions::test_open::open_in(private.path(), &source, HistoryLimits::default())
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
fn native_constraints_applied_and_removed_through_the_session_keep_the_circle_editor_available() {
    if !native() {
        return;
    }
    if !ferritecad_sketch_solver::is_available() {
        ferritecad_fixtures::native_dependencies(&[(false, "PLANEGCS")]);
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let source = create(
        root.path(),
        "create-circle-extrude",
        r#"{"schema_version":1,"center_mm":[12.5,-7.25],"radius_mm":10.5,"height_mm":15.25}"#,
    );
    let opened = crate::sketch_facts::read(&source).expect("reading");
    let sketch = opened.constraint_sketches[0].sketch;
    assert!(opened.circle_sketches[0].refusal.is_none());
    let private = tempfile::tempdir().expect("private root");
    let mut sessions = Sessions::default();
    sessions.adopt(
        crate::sessions::test_open::open_in(private.path(), &source, HistoryLimits::default())
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
        crate::sketch_facts::read(&sessions.export_path().expect("accepted")).expect("reading");
    assert!(constrained.circle_sketches[0].refusal.is_none());
    let accepted = sessions.export_path().expect("accepted");
    let mut form = crate::sketch::Editor::default();
    assert!(form.begin_circle_edit(
        &accepted,
        &constrained,
        constrained.circle_sketches[0].sketch
    ));

    // The same transaction path removes the constraints while keeping the editor available.
    let ids: Vec<_> = stored_constraints(&sessions.export_path().expect("accepted"))
        .iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(ids.len(), 2);
    apply_constraints_native(
        &mut sessions,
        &ferritecad_jobs::EditRequest {
            changes: crate::transaction_tests::constraint_changes(
                &accepted,
                sketch,
                &(ferritecad_document::SketchConstraintEdits {
                    remove: ids,
                    add: Vec::new(),
                }),
            ),
            source: accepted.clone(),
            expected: constrained.version,
            destination: PathBuf::new(),
        },
    );
    let restored =
        crate::sketch_facts::read(&sessions.export_path().expect("accepted")).expect("reading");
    assert!(restored.circle_sketches[0].refusal.is_none());
    form.dismiss();
    assert!(form.begin_circle_edit(
        &sessions.export_path().expect("accepted"),
        &restored,
        restored.circle_sketches[0].sketch
    ));
}

fn requested_circles(
    request: &ferritecad_jobs::EditRequest,
    before: &[(ferritecad_types::StableEntityId, [f64; 2], f64)],
) -> Vec<(ferritecad_types::StableEntityId, [f64; 2], f64)> {
    before
        .iter()
        .map(|&(id, center, radius)| {
            let value = request
                .changes
                .iter()
                .rev()
                .find_map(|change| match change {
                    ferritecad_document::EditChange::SetCurveGeometry {
                        curve,
                        geometry: ferritecad_document::SketchGeometry::Circle { center, radius },
                        ..
                    } if *curve == id => Some(([center.x, center.y], *radius)),
                    _ => None,
                })
                .unwrap_or((center, radius));
            (id, value.0, value.1)
        })
        .collect()
}
