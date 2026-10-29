// SPDX-License-Identifier: MIT
//! §27C: solid full-turn Revolves — a profile closed on the axis along one
//! Line — made by `create-sketch-revolve`, edited by `edit`, and
//! measured after the fact.
//!
//! Expected volumes are written out from cylinder, cone and frustum formulas,
//! never taken from the production Pappus helper. The Line on the axis must
//! raise nothing and carry no name; every other Line keeps its own face.
use super::edit::{check_cells, edit, edit_reply, frustum, rewrite_sketch, saved, write_edit};
use super::*;
use std::collections::BTreeSet;

/// π·r²·h.
fn cylinder(r: f64, h: f64) -> f64 {
    PI * r * r * h
}
/// π·r²·h/3.
fn cone(r: f64, h: f64) -> f64 {
    PI * r * r * h / 3.
}

const CYLINDER: [[f64; 2]; 4] = [[0., 0.], [10., 0.], [10., 15.], [0., 15.]];
const CONE: [[f64; 2]; 3] = [[0., 0.], [10., 0.], [0., 15.]];
const SHAFT: [[f64; 2]; 6] = [
    [0., 0.],
    [10., 0.],
    [10., 5.],
    [6., 5.],
    [6., 15.],
    [0., 15.],
];

/// The index of the Line whose two ends are on the axis.
fn axis_index(points: &[[f64; 2]]) -> usize {
    let n = points.len();
    (0..n)
        .find(|&i| points[i][0] == 0. && points[(i + 1) % n][0] == 0.)
        .expect("one Line on the axis")
}

/// The same profile, starting `rotation` vertices later and optionally
/// drawn the other way round.
fn arranged(points: &[[f64; 2]], rotation: usize, reversed: bool) -> Vec<[f64; 2]> {
    let n = points.len();
    let mut p: Vec<_> = (0..n).map(|i| points[(i + rotation) % n]).collect();
    if reversed {
        p.reverse();
    }
    p
}

/// The kernel's account of a published solid Revolve: volume against the
/// analytic value, faces, and every saved name resolving to the face its Line
/// turns into — with none for the axis Line and nothing at an apex.
fn measure_solid(
    path: &Path,
    points: &[[f64; 2]],
    analytic: f64,
    cache: Option<&[CacheOutcome]>,
) -> f64 {
    let d = Document::open_read_only(path).expect("reopen");
    assert!(d.validate().expect("validate").is_ok());
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = if let Some(expected) = cache {
        let mut store = CacheStore::open(
            path.with_extension("fcad-cache"),
            d.meta().document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("cache");
        let (built, events) =
            ferritecad_eval::rebuild_cached(&d, &mut kernel, &mut store, &context).expect("cached");
        assert_eq!(
            events.iter().map(|e| e.outcome).collect::<Vec<_>>(),
            expected,
            "{events:?}"
        );
        built
    } else {
        ferritecad_eval::rebuild_cold(&d, &mut kernel, &context).expect("cold")
    };
    let objects = d.objects().expect("objects");
    let revolve = objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Revolve(_) => Some(o.id),
            _ => None,
        })
        .expect("revolve");
    let body = objects
        .iter()
        .find_map(|o| matches!(o.payload, ObjectPayload::Body(_)).then_some(o.id))
        .expect("body");
    let shape = built.shape(body).expect("Body");
    let (faces, volume) = kernel.shape_stats(shape).expect("stats");
    assert!(
        (volume - analytic).abs() < 1e-6 * analytic,
        "{volume} != {analytic} for {points:?}"
    );
    assert_eq!(
        faces as usize,
        points.len() - 1,
        "no face for the axis Line"
    );
    let lines: BTreeMap<StableEntityId, ([f64; 2], [f64; 2])> = objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Sketch(s) => Some(
                s.curves
                    .iter()
                    .map(|c| match c.geometry {
                        SketchGeometry::Line { start, end } => {
                            (c.id, ([start.x, start.y], [end.x, end.y]))
                        }
                        _ => panic!("a Line"),
                    })
                    .collect(),
            ),
            _ => None,
        })
        .expect("profile");
    let axis = *lines
        .iter()
        .find(|(_, (a, b))| a[0] == 0. && b[0] == 0.)
        .expect("fixture has a Line on the axis")
        .0;
    let (a, b) = lines[&axis];
    assert_eq!(
        (a[0], b[0]),
        (0., 0.),
        "the stated axis Line is on the axis"
    );
    let refs = crate::names::root_faces(&d, &built);
    assert_eq!(
        refs.len(),
        points.len() - 1,
        "one name per Line off the axis"
    );
    let mesh = kernel
        .tessellate(shape, &TessellationParams::default(), &context)
        .expect("mesh");
    let mut named = BTreeSet::new();
    for (_, input, face) in refs {
        let ferritecad_kernel::NamingInput::Curve(profile_segment) = input else {
            panic!("curve root");
        };
        assert_ne!(profile_segment, axis, "no name for the axis Line");
        assert!(named.insert(face), "two names on one face");
        let (a, b) = lines[&profile_segment];
        let range = mesh.faces.iter().find(|r| r.face == face).expect("drawn");
        let vertices: Vec<[f64; 3]> = mesh.indices
            [range.first_index as usize..(range.first_index + range.index_count) as usize]
            .iter()
            .map(|v| std::array::from_fn(|j| f64::from(mesh.positions[*v as usize * 3 + j])))
            .collect();
        let radius = |p: &[f64; 3]| p[0].hypot(p[2]);
        let surface = kernel.face_surface(face).expect("surface");
        match expected_surface(a, b) {
            "annulus" => {
                // A radial Line from the axis turns into a whole disc.
                assert_eq!(surface, FaceSurface::Plane);
                assert!(vertices.iter().all(|p| (p[1] - a[1]).abs() < 1e-6));
                let outer = a[0].max(b[0]);
                assert!(vertices.iter().all(|p| radius(p) < outer + 1e-6));
            }
            "cylinder" => {
                assert_eq!(surface, FaceSurface::Cylinder { radius: a[0] });
                let (origin, direction) = kernel.surface_axis(face).expect("axis");
                assert!(origin[0].abs() < 1e-9 && origin[2].abs() < 1e-9);
                assert!((direction[1].abs() - 1.).abs() < 1e-12);
                assert!(vertices.iter().all(|p| (radius(p) - a[0]).abs() < 1e-4));
            }
            _ => {
                assert_eq!(surface, FaceSurface::Cone);
                let (origin, direction) = kernel.surface_axis(face).expect("axis");
                assert!(origin[0].abs() < 1e-9 && origin[2].abs() < 1e-9);
                assert!((direction[1].abs() - 1.).abs() < 1e-12);
                for p in &vertices {
                    let t = (p[1] - a[1]) / (b[1] - a[1]);
                    assert!((radius(p) - (a[0] + t * (b[0] - a[0]))).abs() < LINEAR_MM);
                }
            }
        }
    }
    assert_eq!(named.len(), faces as usize, "every face has one name");
    // A name made up for the axis Line resolves to nothing.
    let absent = crate::names::root_name(
        &d,
        revolve,
        ferritecad_kernel::NamingInput::Curve(axis),
        ferritecad_kernel::NamingOperation::Revolve,
    );
    assert_eq!(
        built.topology().features[&revolve]
            .resolve(absent)
            .expect_err("no generated face")
            .kind(),
        ErrorKind::Topology
    );
    built.release_all(&mut kernel);
    d.close().expect("close");
    assert_eq!(kernel.live_shape_count(), 0);
    volume
}

/// The independent STL reading of a solid part: closed and oriented (the
/// shared check), within the analytic band, and actually solid — every radial
/// Line from the axis turns into a disc whose triangles cover π·r², and every
/// apex (a sloped Line ending on the axis) is a mesh vertex on the axis.
fn check_solid_mesh(m: &Mesh, points: &[[f64; 2]], analytic: f64) {
    check_mesh(m, points);
    let n = points.len();
    let band: f64 = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            2. * PI * a[0].max(b[0]) * LINEAR_MM * (b[1] - a[1]).abs()
        })
        .sum();
    assert!(
        (m.volume - analytic).abs() <= band,
        "{} vs {analytic}",
        m.volume
    );
    let area = |t: &[[f64; 3]; 3]| {
        let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
        let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
        let c = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt() / 2.
    };
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        // A radial Line with one end on the axis: a disc, no hole in it.
        if a[1] == b[1] && (a[0] == 0. || b[0] == 0.) {
            let r = a[0].max(b[0]);
            let disc: f64 = m
                .faces
                .iter()
                .filter(|t| t.iter().all(|p| (p[1] - a[1]).abs() < 1e-4))
                .map(area)
                .sum();
            let exact = PI * r * r;
            assert!(
                disc <= exact + 1e-6 && disc >= exact - 2. * PI * r * LINEAR_MM - 1e-3,
                "the disc at y = {} covers {disc} of {exact}",
                a[1]
            );
        }
    }
    // An apex — a vertex on the axis where a sloped Line ends — is a point
    // of the mesh on the axis, not a hole. (A vertex on the axis where a
    // radial Line ends is a disc's centre, covered by the area check above;
    // a disc's triangulation need not have a vertex there.)
    let apexes = (0..n).filter_map(|i| {
        let (a, b) = (points[i], points[(i + 1) % n]);
        let sloped = a[0] != b[0] && a[1] != b[1];
        match (sloped, a[0] == 0., b[0] == 0.) {
            (true, true, false) => Some(a),
            (true, false, true) => Some(b),
            _ => None,
        }
    });
    for p in apexes {
        assert!(
            m.faces
                .iter()
                .flatten()
                .any(|v| v[0].hypot(v[2]) < 1e-4 && (v[1] - p[1]).abs() < 1e-4),
            "no mesh vertex on the axis at y = {}",
            p[1]
        );
    }
}

/// Every Line of the profile, in order, with the saved UUIDs discovery gives.
fn discovered(path: &Path) -> (Value, Vec<String>, String) {
    let c = inspect(path);
    let row = &c["sketches"][0];
    let ids: Vec<String> = row["curves"]
        .as_array()
        .expect("editable vertices")
        .iter()
        .map(|v| v["curve_id"].as_str().expect("id").to_owned())
        .collect();
    assert_eq!(c["revolves"][0]["axis"], "sketch_y");
    assert_eq!(c["revolves"][0]["axis_curve_id"], Value::Null);
    let d = Document::open_read_only(path).expect("document");
    let axis = d
        .objects()
        .expect("objects")
        .into_iter()
        .find_map(|o| {
            let ObjectPayload::Sketch(sketch) = o.payload else {
                return None;
            };
            sketch.curves.into_iter().find_map(|c| match c.geometry {
                SketchGeometry::Line { start, end } if start.x == 0. && end.x == 0. => {
                    Some(c.id.to_string())
                }
                _ => None,
            })
        })
        .expect("fixture axis boundary");
    (c, ids, axis)
}

#[test]
fn native_solid_revolutions_create_measure_name_cache_and_export() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let mut artifact = 0;
    for (name, points, analytic) in [
        ("cylinder", CYLINDER.to_vec(), cylinder(10., 15.)),
        ("cone", CONE.to_vec(), cone(10., 15.)),
        (
            "shaft",
            SHAFT.to_vec(),
            cylinder(10., 5.) + cylinder(6., 10.),
        ),
        (
            "fractional",
            vec![[0., -1.25], [3.5, -1.25], [2.25, 7.75], [0., 7.75]],
            frustum(3.5, 2.25, 9.),
        ),
    ] {
        let n = points.len();
        for (tag, rotation, reversed) in [
            ("as-drawn", 0, false),
            ("rotated", 1, false),
            ("reversed", n - 1, true),
        ] {
            let p = arranged(&points, rotation, reversed);
            let input = d.path().join(format!("{name}-{tag}.json"));
            let out = d.path().join(format!("{name}-{tag}.fcad"));
            request(&input, &p);
            reply(create(&input, &out).output().expect("create"), 0);
            let (c, ids, axis) = discovered(&out);

            let at = axis_index(&p);
            assert_eq!(axis, ids[at], "the axis Line by its own UUID");
            let r = &c["revolves"][0];
            assert!(r.get("closure").is_none());
            assert_eq!(r["axis_curve_id"], Value::Null);

            assert_eq!(c["features"], json!([]));

            let cold = measure_solid(&out, &p, analytic, None);
            assert_eq!(
                cold,
                measure_solid(&out, &p, analytic, Some(&[CacheOutcome::Miss]))
            );
            assert_eq!(
                cold,
                measure_solid(&out, &p, analytic, Some(&[CacheOutcome::Hit]))
            );
            let listed = cli()
                .arg("print-topology")
                .arg(&out)
                .output()
                .expect("print-topology");
            let listed = String::from_utf8(listed.stdout).expect("UTF-8");
            for id in ids.iter().filter(|id| **id != axis) {
                assert!(listed.contains(&format!("curve {id}")));
            }
            assert!(
                crate::names::printed_face_descriptions(&listed)
                    .iter()
                    .all(|description| !description.contains(&format!("curve {axis}")))
            );
            assert!(
                listed
                    .lines()
                    .filter(|line| line.contains(" kind=face "))
                    .count()
                    == n - 1
            );
            let m = mesh(&out, &out.with_extension("stl"));
            check_solid_mesh(&m, &p, analytic);
            if tag == "as-drawn" && artifact < 2 {
                if let Some(dir) = std::env::var_os("FCAD_REVOLVE_AXIS_ARTIFACTS") {
                    let dir = Path::new(&dir);
                    std::fs::create_dir_all(dir).expect("artifacts");
                    let fbx = dir.join(format!("revolve-axis-{artifact}.fbx"));
                    let r = cli()
                        .arg("export-fbx")
                        .arg(&out)
                        .arg("-o")
                        .arg(&fbx)
                        .arg("--json")
                        .output()
                        .expect("FBX");
                    assert!(r.status.success(), "{r:?}");
                }
                artifact += 1;
            }
        }
    }
    assert_eq!(artifact, 2);
    // A part with a bore is still exactly the §27A document.
    let input = d.path().join("bushing.json");
    let out = d.path().join("bushing.fcad");
    request(&input, &BUSHING);
    reply(create(&input, &out).output().expect("create"), 0);
    let c = inspect(&out);

    assert!(c["revolves"][0].get("closure").is_none());
    assert_eq!(c["revolves"][0]["axis_curve_id"], Value::Null);
}

#[test]
fn native_solid_revolution_edits_keep_the_axis_names_sql_and_cache() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    for (name, from, to, from_mm3, to_mm3) in [
        (
            "smaller",
            CYLINDER.to_vec(),
            vec![[0., 2.5], [8., 2.5], [8., 12.5], [0., 12.5]],
            cylinder(10., 15.),
            cylinder(8., 10.),
        ),
        // The outer wall's top end moves in: the same Line, now a cone.
        (
            "to-frustum",
            CYLINDER.to_vec(),
            vec![[0., 0.], [10., 0.], [4., 15.], [0., 15.]],
            cylinder(10., 15.),
            frustum(10., 4., 15.),
        ),
        (
            "cone",
            CONE.to_vec(),
            vec![[0., -2.], [8., -2.], [0., 20.]],
            cone(10., 15.),
            cone(8., 22.),
        ),
        (
            "shaft",
            SHAFT.to_vec(),
            vec![
                [0., 1.],
                [12., 1.],
                [12., 4.],
                [7., 4.],
                [7., 14.],
                [0., 14.],
            ],
            cylinder(10., 5.) + cylinder(6., 10.),
            cylinder(12., 3.) + cylinder(7., 10.),
        ),
    ] {
        for reversed in [false, true] {
            let (from, to) = if reversed {
                (arranged(&from, 0, true), arranged(&to, 0, true))
            } else {
                (from.clone(), to.clone())
            };
            let tag = format!("{name}-{reversed}");
            let input = d.path().join(format!("{tag}.json"));
            let source = d.path().join(format!("{tag}.fcad"));
            request(&input, &from);
            reply(create(&input, &source).output().expect("create"), 0);
            let (before, ids, axis) = discovered(&source);
            measure_solid(&source, &from, from_mm3, Some(&[CacheOutcome::Miss]));
            let s = saved(&source);
            let request_path = d.path().join(format!("{tag}-edit.json"));
            write_edit(&request_path, &ids, &to);
            let bytes = std::fs::read(&source).expect("source");
            let out = d.path().join(format!("{tag} edited.fcad"));
            let v = edit_reply(
                edit(&source, &s.sketch, &s.version, &request_path, &out)
                    .output()
                    .expect("edit"),
                0,
            );
            assert_eq!(v["result"]["document_id"], before["document_id"]);
            assert_eq!(std::fs::read(&source).expect("source"), bytes);
            check_cells(&source, &out, &s.sketch);
            let (after, after_ids, after_axis) = discovered(&out);
            assert_eq!(after_ids, ids);
            assert_eq!(after_axis, axis, "the same axis Line");
            assert!(after["revolves"][0].get("closure").is_none());
            assert_eq!(after["bodies"], before["bodies"]);
            let cold = measure_solid(&out, &to, to_mm3, None);
            // The source's warmed sidecar beside the copy: a Miss, never the
            // old solid.
            std::fs::copy(
                source.with_extension("fcad-cache"),
                out.with_extension("fcad-cache"),
            )
            .expect("stale sidecar");
            assert_eq!(
                cold,
                measure_solid(&out, &to, to_mm3, Some(&[CacheOutcome::Miss]))
            );
            assert_eq!(
                cold,
                measure_solid(&out, &to, to_mm3, Some(&[CacheOutcome::Hit]))
            );
            let m = mesh(&out, &out.with_extension("stl"));
            check_solid_mesh(&m, &to, to_mm3);
        }
    }
}

#[test]
fn native_solid_revolution_refusals_are_atomic() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    collapsed_float_faces_refuse_publication(d.path());
    // Creation: each exits 2 and writes nothing.
    let out = d.path().join("never.fcad");
    let input = d.path().join("refused.json");
    for (why, points, message) in [
        (
            "crossing",
            vec![[-1., 0.], [10., 0.], [10., 15.], [0., 15.]],
            "axis",
        ),
        (
            "self-intersection",
            vec![[0., 0.], [10., 15.], [10., 0.], [0., 15.]],
            "",
        ),
        (
            "degenerate",
            vec![[0., 0.], [0., 5.], [0., 15.], [10., 7.]],
            "",
        ),
    ] {
        request(&input, &points);
        let names = entries(d.path());
        let v = reply(create(&input, &out).output().expect("create"), 2);
        let refusal = v["error"]["message"].as_str().expect("message");
        assert!(refusal.contains(message), "{why}: {refusal}");
        assert_eq!(entries(d.path()), names, "{why}");
    }

    // Edits of a solid cylinder: each exits 2, nothing written, source intact.
    let source = d.path().join("cylinder.fcad");
    request(&input, &CYLINDER);
    reply(create(&input, &source).output().expect("create"), 0);
    let s = saved(&source);
    let request_path = d.path().join("edit.json");
    let refuse = |ids: &[String], points: &[[f64; 2]], version: &str, why: &str| {
        write_edit(&request_path, ids, points);
        let names = entries(d.path());
        let bytes = std::fs::read(&source).expect("source");
        let output = edit(&source, &s.sketch, version, &request_path, &out)
            .output()
            .expect("edit");
        assert_eq!(output.status.code(), Some(2), "{why}: {output:?}");
        let v = edit_reply(output, 2);
        assert_eq!(entries(d.path()), names, "{why}");
        assert_eq!(std::fs::read(&source).expect("source"), bytes, "{why}");
        v["error"].to_string()
    };
    for (why, points, message) in [
        (
            "crossing",
            vec![[0., 0.], [10., 0.], [10., 15.], [-0.5, 15.]],
            "axis",
        ),
        (
            "collapsed axis Line",
            vec![[0., 0.], [10., 0.], [10., 15.], [0., 0.]],
            "",
        ),
    ] {
        let refusal = refuse(&s.ids, &points, &s.version, why);
        assert!(refusal.contains(message), "{why}: {refusal}");
    }
    let stale = ferritecad_types::ContentHash::of_bytes(b"another").to_string();
    let refusal = refuse(&s.ids, &CYLINDER, &stale, "stale");
    assert!(refusal.contains("source has changed"), "{refusal}");
    let mut foreign = s.ids.clone();
    foreign[3] = StableEntityId::new().to_string();
    let refusal = refuse(&foreign, &CYLINDER, &s.version, "foreign axis Line ID");
    assert!(refusal.contains(&foreign[3]), "{refusal}");
    // Destinations that are taken or are the source itself.
    write_edit(&request_path, &s.ids, &CYLINDER);
    let taken = d.path().join("taken.fcad");
    std::fs::write(&taken, b"keep").expect("taken");
    let hard = d.path().join("hard.fcad");
    std::fs::hard_link(&source, &hard).expect("hard link");
    let destinations = [
        ("occupied", taken.clone()),
        ("the source", source.clone()),
        ("hard link to the source", hard),
    ];
    for (why, destination) in destinations {
        let names = entries(d.path());
        let bytes = std::fs::read(&source).expect("source");
        let v = edit_reply(
            edit(&source, &s.sketch, &s.version, &request_path, &destination)
                .output()
                .expect("edit"),
            2,
        );
        let message = v["error"]["message"].as_str().expect("message");
        assert!(
            message.contains("already exists") || message.contains("different files"),
            "{why}: {message}"
        );
        assert_eq!(entries(d.path()), names, "{why}");
        assert_eq!(std::fs::read(&source).expect("source"), bytes, "{why}");
    }
    assert_eq!(std::fs::read(&taken).expect("taken"), b"keep");
    // Coordinate transactions keep constraints and let the solver rebuild them.
    let constrained = d.path().join("constrained.fcad");
    let labels = write_solid_document(&constrained, &CYLINDER);
    rewrite_sketch(&constrained, |sketch| {
        sketch
            .constraints
            .push(ferritecad_document::SketchConstraint {
                id: StableEntityId::new(),
                rule: ferritecad_document::SketchConstraintRule::Horizontal {
                    a: ferritecad_document::SketchPointRef::new(
                        labels[0],
                        ferritecad_document::SketchPointSelector::Start,
                    ),
                    b: ferritecad_document::SketchPointRef::new(
                        labels[0],
                        ferritecad_document::SketchPointSelector::End,
                    ),
                },
            });
    });
    let c = inspect(&constrained);

    let ids: Vec<String> = labels.iter().map(ToString::to_string).collect();
    write_edit(
        &request_path,
        &ids,
        &[[0., 0.], [9., 0.], [9., 15.], [0., 15.]],
    );
    let bytes = std::fs::read(&constrained).expect("constrained");
    let v = edit_reply(
        edit(
            &constrained,
            c["sketches"][0]["sketch_id"].as_str().expect("id"),
            c["content_version"].as_str().expect("version"),
            &request_path,
            &d.path().join("constrained-edited.fcad"),
        )
        .output()
        .expect("edit"),
        if ferritecad_eval::solver_available() {
            0
        } else {
            2
        },
    );
    if ferritecad_eval::solver_available() {
        let copy = d.path().join("constrained-edited.fcad");
        check_cells(
            &constrained,
            &copy,
            c["sketches"][0]["sketch_id"].as_str().expect("Sketch"),
        );
        measure_solid(
            &copy,
            &[[0., 0.], [9., 0.], [9., 15.], [0., 15.]],
            cylinder(9., 15.),
            None,
        );
    } else {
        assert_eq!(v["error"]["kind"], "unsupported");
        assert!(!d.path().join("constrained-edited.fcad").exists());
    }
    assert_eq!(std::fs::read(&constrained).expect("constrained"), bytes);
    // Removing a bore is a topology change, not a forbidden profile class.
    let bushing = d.path().join("bushing.fcad");
    request(&input, &BUSHING);
    reply(create(&input, &bushing).output().expect("create"), 0);
    let b = saved(&bushing);
    write_edit(&request_path, &b.ids, &CYLINDER);
    edit_reply(
        edit(&bushing, &b.sketch, &b.version, &request_path, &out)
            .output()
            .expect("edit"),
        0,
    );
    check_cells(&bushing, &out, &b.sketch);
    measure_solid(&out, &CYLINDER, cylinder(10., 15.), None);
}

// Exercise the global tessellation filter on a pre-existing hollow profile,
// away from any apex. Its two Y levels coincide in the exported f32 mesh.
// A valid B-Rep must not become a successful export with a missing whole face.
fn collapsed_float_faces_refuse_publication(directory: &Path) {
    let input = directory.join("thin.json");
    let source = directory.join("thin.fcad");
    request(
        &input,
        &[
            [4., 100_000.],
            [10., 100_000.],
            [10., 100_000.002],
            [4., 100_000.002],
        ],
    );
    reply(create(&input, &source).output().expect("thin B-Rep"), 0);
    let bytes = std::fs::read(&source).expect("source");
    for (operation, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
        let destination = directory.join(format!("thin.{extension}"));
        let names = entries(directory);
        let output = cli()
            .arg(operation)
            .arg(&source)
            .arg("-o")
            .arg(&destination)
            .arg("--json")
            .output()
            .expect("export");
        assert_eq!(output.status.code(), Some(2), "{operation}: {output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).expect("JSON");
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["kind"], "kernel");
        assert!(!destination.exists(), "{operation} must not omit a face");
        assert_eq!(entries(directory), names, "no output or scratch");
        assert_eq!(std::fs::read(&source).expect("source"), bytes);
    }
}

/// A solid Revolve document written without any kernel, the way §27A's
/// kernel-free test writes a bored one: the stated axis Line in the payload,
/// and one RevolveFace per other Line.
pub(super) fn write_solid_document(path: &Path, points: &[[f64; 2]]) -> Vec<StableEntityId> {
    let mut d = Document::create(path).expect("document");
    let [plane, sketch, revolve, body] = std::array::from_fn(|_| ObjectId::new());
    let n = points.len();
    let labels: Vec<StableEntityId> = (0..n).map(|_| StableEntityId::new()).collect();
    d.write(|w| {
        w.put_object(
            plane,
            None,
            0,
            Some("XY"),
            &ObjectPayload::DatumPlane(DatumPlane {
                placement: Transform::IDENTITY,
            }),
        )?;
        let curves = (0..n)
            .map(|i| {
                Ok(SketchCurve {
                    id: labels[i],
                    construction: false,
                    geometry: SketchGeometry::Line {
                        start: Point2::new(points[i][0], points[i][1])?,
                        end: Point2::new(points[(i + 1) % n][0], points[(i + 1) % n][1])?,
                    },
                })
            })
            .collect::<ferritecad_types::Result<Vec<_>>>()?;
        w.put_object(
            sketch,
            None,
            1,
            Some("Profile"),
            &ObjectPayload::Sketch(Sketch {
                plane,
                curves,
                constraints: Vec::new(),
            }),
        )?;
        w.put_object(
            revolve,
            None,
            2,
            Some("Revolve1"),
            &ObjectPayload::Revolve(Revolve {
                regions: Vec::new(),
                profile: sketch,
                axis: RevolveAxis::SketchY,
                extent: RevolveExtent::FullTurn,
                operation: SolidOperation::NewBody,
                previous: None,
            }),
        )?;
        w.put_object(
            body,
            None,
            3,
            Some("Body"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(revolve),
            }),
        )?;
        for (dependent, dependency, role) in [
            (sketch, plane, DependencyRole::Plane),
            (revolve, sketch, DependencyRole::Profile),
            (body, revolve, DependencyRole::BodyTip),
        ] {
            w.add_dependency(Dependency {
                dependent,
                dependency,
                role,
            })?;
        }
        Ok(())
    })
    .expect("solid document");
    d.close().expect("close");
    labels
}

/// A kernel and no solver: a solid part needs no planegcs, to make or edit.
#[test]
fn occt_without_solver_builds_and_edits_a_solid_revolve() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("cone.json");
    let source = d.path().join("cone.fcad");
    request(&input, &CONE);
    reply(create(&input, &source).output().expect("create"), 0);
    measure_solid(&source, &CONE, cone(10., 15.), None);
    let s = saved(&source);
    let to = [[0., -2.], [8., -2.], [0., 20.]];
    let request_path = d.path().join("edit.json");
    write_edit(&request_path, &s.ids, &to);
    let out = d.path().join("edited.fcad");
    edit_reply(
        edit(&source, &s.sketch, &s.version, &request_path, &out)
            .output()
            .expect("edit"),
        0,
    );
    measure_solid(&out, &to, cone(8., 22.), None);
}

#[test]
fn native_solid_revolve_delivery_late_guards_and_cancellation() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("shaft.json");
    request(&input, &SHAFT);
    // Exit 7 on creation: the report is lost, the solid part stands.
    for both in [false, true] {
        let published = d.path().join(format!("created-{both}.fcad"));
        let mut c = create(&input, &published);
        c.stdout(pipe::closed_pipe());
        if both {
            c.stderr(pipe::closed_pipe());
        } else {
            c.stderr(std::process::Stdio::piped());
        }
        assert_eq!(c.output().expect("closed").status.code(), Some(7));
        measure_solid(
            &published,
            &SHAFT,
            cylinder(10., 5.) + cylinder(6., 10.),
            None,
        );
    }
    let source = d.path().join("created-false.fcad");
    let s = saved(&source);
    let to = [
        [0., 1.],
        [12., 1.],
        [12., 4.],
        [7., 4.],
        [7., 14.],
        [0., 14.],
    ];
    let to_mm3 = cylinder(12., 3.) + cylinder(7., 10.);
    let request_path = d.path().join("edit.json");
    write_edit(&request_path, &s.ids, &to);
    // Exit 7 on the edit.
    let published = d.path().join("edited-pipe.fcad");
    let mut c = edit(&source, &s.sketch, &s.version, &request_path, &published);
    c.stdout(pipe::closed_pipe())
        .stderr(std::process::Stdio::piped());
    assert_eq!(c.output().expect("closed").status.code(), Some(7));
    measure_solid(&published, &to, to_mm3, None);
    // The shared job with the kernel that ships, at its own phases.
    let reading = ferritecad_jobs::read_extrude_source(&source).expect("reading");
    let source_doc = Document::open_read_only(&source).expect("source");
    let (sketch_id, saved_sketch) = ferritecad_fixtures::sketch(&source_doc).expect("sketch");
    source_doc.close().expect("close");
    let vertices: Vec<ferritecad_document::SketchVertex> = saved_sketch
        .curves
        .iter()
        .zip(to)
        .map(|(v, p)| ferritecad_document::SketchVertex {
            curve_id: v.id,
            start_mm: p,
        })
        .collect();
    let job = |name: &str| ferritecad_jobs::EditRequest {
        changes: {
            let document =
                ferritecad_document::Document::open_read_only(source.clone()).expect("source");
            ferritecad_fixtures::vertex_changes(&document, sketch_id, &(vertices.clone()))
                .expect("changes")
        },
        source: source.clone(),
        expected: reading.version,
        destination: d.path().join(name),
    };
    let names = entries(d.path());
    let token = ferritecad_kernel::CancelToken::new();
    let stop = token.clone();
    let context = OperationContext::default()
        .with_cancel(token)
        .with_progress(ferritecad_kernel::ProgressSink::new(move |f| {
            if f >= 0.4 {
                stop.cancel();
            }
        }));
    let error = ferritecad_jobs::edit_copy(
        &job("cancelled.fcad"),
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &context,
    )
    .expect_err("cancelled");
    assert_eq!(error.kind(), ErrorKind::Cancellation);
    assert_eq!(entries(d.path()), names, "no copy and no scratch");
    // Cancelled after publication: published anyway.
    let token = ferritecad_kernel::CancelToken::new();
    let stop = token.clone();
    let context = OperationContext::default()
        .with_cancel(token)
        .with_progress(ferritecad_kernel::ProgressSink::new(move |f| {
            if f >= 1.0 {
                stop.cancel();
            }
        }));
    let late = job("after.fcad");
    ferritecad_jobs::edit_copy(
        &late,
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &context,
    )
    .expect("published before the cancellation");
    measure_solid(&late.destination, &to, to_mm3, None);
}
