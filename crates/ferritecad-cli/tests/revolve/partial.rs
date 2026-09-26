// SPDX-License-Identifier: MIT
//! §27D: partial Revolves — the same Line profile, with a bore or closed on
//! the axis, turned through a stated angle — made by `create-sketch-revolve`
//! request v2 and measured after the fact.
//!
//! Expected volumes are the full-turn formulas × θ/360, written out here and
//! never taken from the production Pappus helper. Each end face is checked for
//! its plane, its outward side and its area against the profile itself, once
//! from the kernel's own mesh and again from an independent reading of the
//! exported STL. A full turn does not pass these checks: it has no end faces
//! and reaches every angle.
use super::edit::{edit, edit_reply, write_edit};
use super::*;
use ferritecad_document::{CapSide, RevolveAngle};
use std::collections::BTreeSet;

const CYLINDER: [[f64; 2]; 4] = [[0., 0.], [10., 0.], [10., 15.], [0., 15.]];
const CONE: [[f64; 2]; 3] = [[0., 0.], [10., 0.], [0., 15.]];
/// Fractional radii and heights, shifted along Y, with a bore.
const FRACTIONAL: [[f64; 2]; 5] = [
    [2.75, -3.25],
    [7.5, -3.25],
    [7.5, 2.125],
    [4.25, 9.5],
    [2.75, 9.5],
];

/// This test's own Pappus: 2π ∫∫ x dA, from the profile's own edges.
pub(super) fn pappus(points: &[[f64; 2]]) -> f64 {
    let n = points.len();
    let moment: f64 = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            (b[1] - a[1]) * (a[0] * a[0] + a[0] * b[0] + b[0] * b[0]) / 6.
        })
        .sum();
    2. * PI * moment.abs()
}

/// The profile's own area, by the shoelace formula.
pub(super) fn profile_area(points: &[[f64; 2]]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        .abs()
        / 2.
}

/// Whether (r, y) is inside the profile or within `tolerance` of its edge.
fn in_profile(points: &[[f64; 2]], r: f64, y: f64, tolerance: f64) -> bool {
    let n = points.len();
    let mut odd = false;
    let mut near = f64::INFINITY;
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        if (a[1] > y) != (b[1] > y) && r < a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]) {
            odd = !odd;
        }
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let t = (((r - a[0]) * dx + (y - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0., 1.);
        near = near.min((r - a[0] - t * dx).hypot(y - a[1] - t * dy));
    }
    odd || near < tolerance
}

/// The same profile starting `rotation` vertices later, optionally drawn the
/// other way round.
fn arranged(points: &[[f64; 2]], rotation: usize, reversed: bool) -> Vec<[f64; 2]> {
    let n = points.len();
    let mut p: Vec<_> = (0..n).map(|i| points[(i + rotation) % n]).collect();
    if reversed {
        p.reverse();
    }
    p
}

pub(super) fn axis_line(points: &[[f64; 2]]) -> Option<usize> {
    let n = points.len();
    (0..n).find(|&i| points[i][0] == 0. && points[(i + 1) % n][0] == 0.)
}

/// The turn angle of a model point, in degrees in [0, 360): a profile point
/// (x, y) turned by φ is (x·cos φ, y, −x·sin φ).
fn phi(p: &[f64; 3]) -> f64 {
    let a = (-p[2]).atan2(p[0]).to_degrees();
    if a < 0. { a + 360. } else { a }
}

/// A turn angle measured from 0, with a wrap within 0.001° of 360 read as
/// just below 0, so rounding at the start plane is not taken for a full turn.
/// Narrower than the 0.01° the widest sector leaves open, so the end plane of
/// a 359.99° sector is never read as its start.
fn signed_phi(p: &[f64; 3]) -> f64 {
    let a = phi(p);
    if a > 360. - 1e-3 { a - 360. } else { a }
}

/// The outward normal and in-plane radial direction of one end face.
fn cap_frame(side: CapSide, degrees: f64) -> ([f64; 3], [f64; 3]) {
    let (s, c) = degrees.to_radians().sin_cos();
    match side {
        CapSide::Start => ([0., 0., 1.], [1., 0., 0.]),
        _ => ([-s, 0., -c], [c, 0., -s]),
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(t: &[[f64; 3]; 3]) -> [f64; 3] {
    let u = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
    let v = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

fn length(v: [f64; 3]) -> f64 {
    dot(v, v).sqrt()
}

pub(super) fn request_v2(path: &Path, points: &[[f64; 2]], extent: Value) {
    write(
        path,
        &json!({"request_version":2,"points_mm":points,"axis":"sketch_y","extent":extent}),
    );
}

pub(super) fn angle(degrees: f64) -> Value {
    json!({"kind":"angle","degrees":degrees})
}

/// The kernel's account of a published sector: volume against the full-turn
/// formula × θ/360, one face per non-axis Line plus two end faces, and every
/// stored name resolving to the face it means — each face of revolution
/// turned exactly from the start plane to the end plane, each end face on its
/// plane, facing out, with the profile's area.
pub(super) fn measure_partial(
    path: &Path,
    points: &[[f64; 2]],
    degrees: f64,
    full: f64,
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
    assert_eq!(built.shape_count(), 1);
    let objects = d.objects().expect("objects");
    let (revolve, body) = objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Body(b) => Some((b.tip_feature.expect("tip"), o.id)),
            _ => None,
        })
        .expect("body");
    let shape = built.shape(body).expect("Body");
    let (faces, volume) = kernel.shape_stats(shape).expect("stats");
    let expected = full * degrees / 360.;
    assert!(
        (volume - expected).abs() < 1e-9 * expected,
        "{volume} != {expected} for {points:?} through {degrees}°"
    );
    let axis = axis_line(points);
    let raising = points.len() - usize::from(axis.is_some());
    assert_eq!(faces as usize, raising + 2, "Line faces and two end faces");
    let sketch: Vec<(StableEntityId, [f64; 2], [f64; 2])> = objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Sketch(s) => Some(
                s.curves
                    .iter()
                    .map(|c| match c.geometry {
                        SketchGeometry::Line { start, end } => {
                            (c.id, [start.x, start.y], [end.x, end.y])
                        }
                        _ => panic!("a Line"),
                    })
                    .collect(),
            ),
            _ => None,
        })
        .expect("profile");
    let axis_label = axis.map(|i| sketch[i].0);
    let lines: BTreeMap<_, _> = sketch.iter().map(|(id, a, b)| (*id, (*a, *b))).collect();
    let mesh = kernel
        .tessellate(shape, &TessellationParams::default(), &context)
        .expect("mesh");
    let triangles = |face| -> Vec<[[f64; 3]; 3]> {
        let range = mesh.faces.iter().find(|r| r.face == face).expect("drawn");
        mesh.indices[range.first_index as usize..(range.first_index + range.index_count) as usize]
            .chunks_exact(3)
            .map(|t| {
                std::array::from_fn(|k| {
                    std::array::from_fn(|j| f64::from(mesh.positions[t[k] as usize * 3 + j]))
                })
            })
            .collect()
    };
    let area = profile_area(points);
    let refs = d.topology_refs().expect("refs");
    assert_eq!(refs.len(), raising + 2, "one name per face");
    let mut named = BTreeSet::new();
    let mut caps = BTreeMap::new();
    for reference in &refs {
        let resolved = built.resolve(reference).expect("resolves");
        let [face] = resolved.as_slice() else {
            panic!("one face for {:?}: {resolved:?}", reference.output_role)
        };
        assert!(named.insert(*face), "two names on one face");
        let t = triangles(*face);
        let vertices: Vec<[f64; 3]> = t.iter().flatten().copied().collect();
        let surface = kernel.face_surface(*face).expect("surface");
        match reference.output_role {
            SemanticRole::RevolveFace { profile_segment } => {
                assert_ne!(
                    Some(profile_segment),
                    axis_label,
                    "the axis Line has no face"
                );
                assert_eq!(
                    reference.selection,
                    SelectionRule::AllDerivedFrom {
                        ancestor: profile_segment
                    }
                );
                let (a, b) = lines[&profile_segment];
                let radius = |p: &[f64; 3]| p[0].hypot(p[2]);
                match expected_surface(a, b) {
                    "annulus" => {
                        assert_eq!(surface, FaceSurface::Plane);
                        assert!(vertices.iter().all(|p| (p[1] - a[1]).abs() < 1e-6));
                    }
                    "cylinder" => {
                        assert_eq!(surface, FaceSurface::Cylinder { radius: a[0] });
                        assert!(vertices.iter().all(|p| (radius(p) - a[0]).abs() < 1e-4));
                    }
                    _ => assert_eq!(surface, FaceSurface::Cone),
                }
                // Turned from the start plane exactly to the end plane.
                let angles: Vec<f64> = vertices
                    .iter()
                    .filter(|p| radius(p) > 1e-3)
                    .map(signed_phi)
                    .collect();
                let lo = angles.iter().copied().fold(f64::INFINITY, f64::min);
                let hi = angles.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                assert!(lo.abs() < 1e-3, "starts at {lo}°, not 0");
                assert!((hi - degrees).abs() < 1e-3, "ends at {hi}°, not {degrees}");
            }
            SemanticRole::RevolveCap { side } => {
                assert_eq!(reference.selection, SelectionRule::Exact);
                assert_eq!(reference.producer_feature, revolve);
                assert_eq!(reference.owner, revolve);
                assert_eq!(surface, FaceSurface::Plane);
                let (outward, radial) = cap_frame(side, degrees);
                for p in &vertices {
                    assert!(
                        dot(*p, outward).abs() < 1e-5,
                        "{p:?} off the {side:?} plane"
                    );
                    let r = dot(*p, radial);
                    assert!(
                        in_profile(points, r, p[1], 1e-5),
                        "{p:?} is outside the profile on the {side:?} face"
                    );
                }
                let sum = t
                    .iter()
                    .map(cross)
                    .fold([0.; 3], |s, c| [s[0] + c[0], s[1] + c[1], s[2] + c[2]]);
                let covered: f64 = t.iter().map(|x| length(cross(x)) / 2.).sum();
                assert!(
                    dot(sum, outward) / length(sum) > 1. - 1e-9,
                    "the {side:?} face does not face out: {sum:?}"
                );
                assert!(
                    (covered - area).abs() < 1e-5 * area,
                    "the {side:?} face covers {covered} of {area}"
                );
                assert!(caps.insert(side, *face).is_none());
            }
            ref other => panic!("unexpected role {other:?}"),
        }
    }
    assert_eq!(named.len(), faces as usize, "every face has one name");
    assert_eq!(caps.len(), 2);
    // Neither an extrusion's cap nor a Revolve cap selected by ancestry, nor
    // a face of the axis Line, resolves on a sector.
    let (label, _) = lines.iter().next().expect("a Line");
    for (role, selection) in [
        (
            SemanticRole::ExtrudeCap {
                side: CapSide::Start,
            },
            SelectionRule::Exact,
        ),
        (
            SemanticRole::RevolveCap { side: CapSide::End },
            SelectionRule::AllDerivedFrom { ancestor: *label },
        ),
    ] {
        let wrong = TopologyRef {
            id: StableEntityId::new(),
            owner: revolve,
            producer_feature: revolve,
            expected_kind: EntityKind::Face,
            output_role: role,
            selection,
            fallback_signature: None,
        };
        assert!(built.resolve(&wrong).is_err(), "{wrong:?}");
    }
    if let Some(axis) = axis_label {
        let fake = TopologyRef {
            id: StableEntityId::new(),
            owner: revolve,
            producer_feature: revolve,
            expected_kind: EntityKind::Face,
            output_role: SemanticRole::RevolveFace {
                profile_segment: axis,
            },
            selection: SelectionRule::AllDerivedFrom { ancestor: axis },
            fallback_signature: None,
        };
        assert!(built.resolve(&fake).is_err(), "the axis Line has no face");
    }
    built.release_all(&mut kernel);
    d.close().expect("close");
    assert_eq!(kernel.live_shape_count(), 0);
    volume
}

/// The independent STL reading of a sector: one closed, outward surface of
/// the right volume, every vertex inside the turned profile and inside the
/// angle — nothing in the negative space beyond it — and two planar end faces
/// on their planes, facing out, each covering the profile's area.
pub(super) fn check_partial_mesh(m: &Mesh, points: &[[f64; 2]], degrees: f64, full: f64) {
    closed_through_t_junctions(m);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in m.faces.iter().flatten() {
        let r = v[0].hypot(v[2]);
        assert!(
            in_profile(points, r, v[1], LINEAR_MM + ROUNDING_MM),
            "{v:?} is outside the turned profile"
        );
        if r > 1e-3 {
            let a = signed_phi(v);
            assert!(
                a > -1e-3 && a < degrees + 1e-3,
                "{v:?} at {a}° lies outside the {degrees}° sector"
            );
            lo = lo.min(a);
            hi = hi.max(a);
        }
    }
    assert!(lo.abs() < 1e-3 && (hi - degrees).abs() < 1e-3, "{lo}..{hi}");
    // Chord deviation only moves the faces of revolution, and only inward, by
    // at most LINEAR_MM; the end faces are exact planes.
    let exact = full * degrees / 360.;
    let band: f64 = (0..points.len())
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            2. * PI * a[0].max(b[0]) * LINEAR_MM * (b[1] - a[1]).abs()
        })
        .sum::<f64>()
        * degrees
        / 360.;
    assert!(m.volume > 0., "the surface faces inward: {}", m.volume);
    assert!(
        (m.volume - exact).abs() <= band,
        "mesh volume {} not within {band} of {exact}",
        m.volume
    );
    let area = profile_area(points);
    for side in [CapSide::Start, CapSide::End] {
        let (outward, radial) = cap_frame(side, degrees);
        let covered: f64 = m
            .faces
            .iter()
            .filter(|t| {
                let c = cross(t);
                t.iter()
                    .all(|p| dot(*p, outward).abs() < 1e-4 && dot(*p, radial) > -1e-4)
                    && length(c) > 0.
                    && dot(c, outward) / length(c) > 1. - 1e-6
            })
            .map(|t| length(cross(t)) / 2.)
            .sum();
        assert!(
            (covered - area).abs() < 1e-4 * area,
            "the STL's {side:?} face covers {covered} of {area}"
        );
    }
}

/// Closed and consistently oriented, allowing only T-junctions.
///
/// Every directed edge appears once. An edge without its reverse is accepted
/// only where mesh vertices lie on it: split there, every piece must meet its
/// reverse among the other split edges. Measured on OCCT 8.0.1: a cone sector
/// is meshed with collinear slivers along its first ruling, and the ones whose
/// float area is exactly zero are omitted (§27C) because STL cannot write them.
/// What remains is geometrically closed, with vertices lying on the edges of
/// the kept slivers; a real hole has nothing on its edge and still fails.
fn closed_through_t_junctions(m: &Mesh) {
    let q = |v: [f64; 3]| v.map(|x| (x * 1e4).round() as i64);
    let mut directed: BTreeMap<_, usize> = BTreeMap::new();
    let mut at = BTreeMap::new();
    for face in &m.faces {
        let p: Vec<_> = face.iter().map(|v| q(*v)).collect();
        for (k, v) in p.iter().enumerate() {
            at.insert(*v, face[k]);
        }
        for k in 0..3 {
            *directed.entry((p[k], p[(k + 1) % 3])).or_default() += 1;
        }
    }
    assert!(
        directed.values().all(|n| *n == 1),
        "not one oriented surface"
    );
    let open: Vec<_> = directed
        .keys()
        .filter(|(a, b)| !directed.contains_key(&(*b, *a)))
        .copied()
        .collect();
    let mut pieces: BTreeMap<_, isize> = BTreeMap::new();
    for (a, b) in &open {
        let (pa, pb) = (at[a], at[b]);
        let d = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
        let len2 = dot(d, d);
        let mut on: Vec<(f64, [i64; 3])> = at
            .iter()
            .filter(|(k, _)| *k != a && *k != b)
            .filter_map(|(k, p)| {
                let w = [p[0] - pa[0], p[1] - pa[1], p[2] - pa[2]];
                let t = dot(w, d) / len2;
                let off = [w[0] - t * d[0], w[1] - t * d[1], w[2] - t * d[2]];
                (t > 0. && t < 1. && length(off) < 1e-4).then_some((t, *k))
            })
            .collect();
        on.sort_by(|x, y| x.0.total_cmp(&y.0));
        let chain: Vec<[i64; 3]> = std::iter::once(*a)
            .chain(on.into_iter().map(|(_, k)| k))
            .chain(std::iter::once(*b))
            .collect();
        for w in chain.windows(2) {
            *pieces.entry((w[0], w[1])).or_default() += 1;
            *pieces.entry((w[1], w[0])).or_default() -= 1;
        }
    }
    assert!(
        pieces.values().all(|n| *n == 0),
        "open mesh: {} unmatched edges that no T-junction explains",
        open.len()
    );
}

pub(super) fn payload_version(path: &Path) -> i64 {
    let sql =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("SQL");
    sql.query_row(
        "SELECT schema_version FROM objects WHERE kind='feature.revolve'",
        [],
        |r| r.get(0),
    )
    .expect("version")
}

pub(super) fn expected_capabilities(solid: bool) -> Vec<String> {
    let mut names = vec![
        "core.part.v1",
        "feature.revolve.partial.v1",
        "feature.revolve.v1",
    ];
    if solid {
        names.insert(1, "feature.revolve.axis-closed.v1");
    }
    names.into_iter().map(str::to_owned).collect()
}

#[test]
fn native_partial_revolutions_measure_caps_names_cache_and_exports() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let mut artifact = 0;
    for (name, points, full) in [
        ("bushing", BUSHING.to_vec(), PI * (100. - 16.) * 15.),
        ("cylinder", CYLINDER.to_vec(), PI * 100. * 15.),
        ("cone", CONE.to_vec(), PI * 100. * 15. / 3.),
        ("fractional", FRACTIONAL.to_vec(), pappus(&FRACTIONAL)),
    ] {
        let n = points.len();
        for degrees in [90., 180., 270., 137.5] {
            let every = degrees == 90. || degrees == 137.5;
            for (tag, rotation, reversed) in [
                ("as-drawn", 0, false),
                ("rotated", 1, false),
                ("reversed", n - 1, true),
            ] {
                if tag != "as-drawn" && !every {
                    continue;
                }
                let p = arranged(&points, rotation, reversed);
                let solid = axis_line(&p).is_some();
                let input = d.path().join(format!("{name}-{degrees}-{tag}.json"));
                let out = d.path().join(format!("{name}-{degrees}-{tag}.fcad"));
                request_v2(&input, &p, angle(degrees));
                reply(create(&input, &out).output().expect("create"), 0);

                let c = inspect(&out);
                assert_eq!(c["features"], json!([]));
                let [r] = c["revolves"].as_array().expect("revolves").as_slice() else {
                    panic!("one Revolve")
                };
                assert_eq!(r["extent"], "partial_turn");
                assert_eq!(r["angle_deg"], json!(degrees));
                assert_eq!(r["axis"], "sketch_y");
                assert_eq!(r["operation"], "new_body");
                assert_eq!(r["profile"]["available"], true);
                assert_eq!(
                    r["closure"],
                    if solid { "axis_closed" } else { "radial_clear" }
                );
                let ids: Vec<String> = r["profile"]["segments"]
                    .as_array()
                    .expect("segments")
                    .iter()
                    .map(|s| s["curve_id"].as_str().expect("id").to_owned())
                    .collect();
                assert_eq!(ids.len(), n);
                let axis_id = axis_line(&p).map(|i| ids[i].clone());
                assert_eq!(r["axis_curve_id"], json!(axis_id));
                let row = &c["sketches"][0];
                assert_eq!(row["editable"], false);
                assert!(
                    row["refusal"]
                        .as_str()
                        .expect("refusal")
                        .contains("partial Revolve"),
                    "{row}"
                );
                assert_eq!(row["vertices"], Value::Null);
                assert_eq!(row["profile_feature"], Value::Null);
                assert_eq!(capability_rows(&out), expected_capabilities(solid));
                assert_eq!(payload_version(&out), if solid { 4 } else { 3 });

                let cold = measure_partial(&out, &p, degrees, full, None);
                assert_eq!(
                    cold,
                    measure_partial(&out, &p, degrees, full, Some(&[CacheOutcome::Miss]))
                );
                assert_eq!(
                    cold,
                    measure_partial(&out, &p, degrees, full, Some(&[CacheOutcome::Hit]))
                );
                let listed = cli()
                    .arg("print-topology")
                    .arg(&out)
                    .output()
                    .expect("print-topology");
                let listed = String::from_utf8(listed.stdout).expect("UTF-8");
                for id in &ids {
                    assert_eq!(
                        listed.contains(&format!("revolve face from segment {id}")),
                        Some(id) != axis_id.as_ref(),
                        "{id}"
                    );
                }
                assert!(listed.contains("revolve start cap"));
                assert!(listed.contains("revolve end cap"));
                let named = ids.len() - usize::from(solid) + 2;
                assert!(listed.contains(&format!("{named} of {named} references resolved")));
                let m = mesh(&out, &out.with_extension("stl"));
                check_partial_mesh(&m, &p, degrees, full);
                if tag == "as-drawn"
                    && ((name == "bushing" && degrees == 90.)
                        || (name == "cone" && degrees == 270.)
                        || (name == "fractional" && degrees == 137.5))
                {
                    if let Some(dir) = std::env::var_os("FCAD_REVOLVE_PARTIAL_ARTIFACTS") {
                        let dir = Path::new(&dir);
                        std::fs::create_dir_all(dir).expect("artifacts");
                        // Both at the default tessellation, the only one
                        // export-fbx has, so the CI join compares one mesh.
                        for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
                            let target =
                                dir.join(format!("revolve-partial-{artifact}.{extension}"));
                            let r = cli()
                                .arg(op)
                                .arg(&out)
                                .arg("-o")
                                .arg(&target)
                                .arg("--json")
                                .output()
                                .expect("artifact");
                            assert!(r.status.success(), "{r:?}");
                        }
                    }
                    artifact += 1;
                }
            }
        }
    }
    assert_eq!(artifact, 3);
    // Both ends of the accepted range are sectors, not a refusal and not a
    // full turn, and a fraction that has no finite decimal is kept exactly.
    for degrees in [
        RevolveAngle::MIN_DEGREES,
        RevolveAngle::MAX_DEGREES,
        100. / 3.,
    ] {
        let input = d.path().join(format!("edge-{degrees}.json"));
        let out = d.path().join(format!("edge-{degrees}.fcad"));
        request_v2(&input, &BUSHING, angle(degrees));
        reply(create(&input, &out).output().expect("create"), 0);
        assert_eq!(inspect(&out)["revolves"][0]["angle_deg"], json!(degrees));
        measure_partial(&out, &BUSHING, degrees, PI * (100. - 16.) * 15., None);
    }
}

#[test]
fn partial_revolution_requests_refuse_every_other_turn_atomically() {
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("request.json");
    let out = d.path().join("out.fcad");
    for (body, kind, why) in [
        (angle(0.), "input", "zero"),
        (
            json!({"kind":"angle","degrees":-0.0}),
            "input",
            "minus zero",
        ),
        (angle(-90.), "input", "a mirrored sweep"),
        (angle(0.005), "input", "thinner than 0.01°"),
        (angle(359.995), "input", "within 0.01° of a full turn"),
        (angle(360.), "input", "exactly a full turn"),
        (angle(400.), "input", "more than one turn"),
        (angle(720.), "input", "two turns"),
        (json!({"kind":"angle","degrees":"90"}), "input", "a string"),
        (json!({"kind":"angle"}), "input", "no degrees"),
        (
            json!({"kind":"angle","degrees":90,"direction":"cw"}),
            "input",
            "an extra field",
        ),
        (
            json!({"kind":"full_turn","degrees":90}),
            "input",
            "an angle on a full turn",
        ),
        (json!({"kind":"half_turn"}), "input", "an unknown kind"),
        (json!("full_turn"), "input", "v1's spelling as an extent"),
    ] {
        request_v2(&input, &BUSHING, body.clone());
        let names = entries(d.path());
        let v = reply(create(&input, &out).output().expect("process"), 2);
        assert_eq!(v["error"]["kind"], kind, "{why}: {v}");
        assert_eq!(entries(d.path()), names, "{why}");
    }
    let message = |value: &Value| {
        write(&input, value);
        let v = reply(create(&input, &out).output().expect("process"), 2);
        v["error"]["message"].as_str().expect("message").to_owned()
    };
    let text = message(
        &json!({"request_version":2,"points_mm":BUSHING,"axis":"sketch_y",
        "extent":{"kind":"angle","degrees":360}}),
    );
    assert!(text.contains("full turn"), "{text}");
    let text = message(
        &json!({"request_version":2,"points_mm":BUSHING,"axis":"sketch_y",
        "extent":{"kind":"angle","degrees":1e-3}}),
    );
    assert!(text.contains("0.01"), "{text}");
    for (value, why) in [
        (
            json!({"request_version":2,"points_mm":BUSHING,"axis":"sketch_y","angle":"full_turn"}),
            "v1's angle field in v2",
        ),
        (
            json!({"request_version":2,"points_mm":BUSHING,"axis":"sketch_y"}),
            "no extent",
        ),
        (
            json!({"request_version":2,"points_mm":BUSHING,"axis":"sketch_y",
                "extent":{"kind":"full_turn"},"angle":"full_turn"}),
            "both spellings",
        ),
        (
            json!({"request_version":1,"points_mm":BUSHING,"axis":"sketch_y","angle":"full_turn",
                "extent":{"kind":"angle","degrees":90}}),
            "an extent in v1",
        ),
        (
            json!({"request_version":1,"points_mm":BUSHING,"axis":"sketch_y","angle":"half_turn"}),
            "a partial angle in v1",
        ),
        (
            json!({"request_version":2,"points_mm":[[0,0],[10,0],[10,15],[4,15]],
                "axis":"sketch_y","extent":{"kind":"angle","degrees":90}}),
            "a profile the shared policy refuses",
        ),
    ] {
        write(&input, &value);
        let names = entries(d.path());
        let v = reply(create(&input, &out).output().expect("process"), 2);
        assert_eq!(v["error"]["kind"], "input", "{why}: {v}");
        assert_eq!(entries(d.path()), names, "{why}");
    }
    if !ferritecad_occt::is_available() {
        // A valid sector is still only proven by building it: a build without
        // a kernel publishes nothing.
        request_v2(&input, &BUSHING, angle(90.));
        let names = entries(d.path());
        let v = reply(create(&input, &out).output().expect("stub process"), 2);
        assert_eq!(v["error"]["kind"], "unsupported", "{v}");
        assert_eq!(entries(d.path()), names);
    }
}

#[test]
fn native_partial_revolve_guards_delivery_and_cancellation() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("request.json");
    request_v2(&input, &CONE, angle(137.5));
    let saved_request = std::fs::read(&input).expect("request");
    // Never over an existing file, and never over the request itself.
    let taken = d.path().join("taken.fcad");
    std::fs::write(&taken, b"keep").expect("occupied");
    let alias = d.path().join("alias.fcad");
    std::fs::hard_link(&input, &alias).expect("hard link");
    for dest in [&taken, &input, &alias] {
        let names = entries(d.path());
        reply(create(&input, dest).output().expect("process"), 2);
        assert_eq!(entries(d.path()), names);
    }
    assert_eq!(std::fs::read(&taken).expect("taken"), b"keep");
    assert_eq!(std::fs::read(&input).expect("request"), saved_request);
    // A report that cannot be delivered after publication is exit 7, and the
    // sector it published is intact.
    let published = d.path().join("pipe.fcad");
    let delivery = create(&input, &published)
        .stdout(pipe::closed_pipe())
        .stderr(pipe::closed_pipe())
        .status()
        .expect("closed stdout");
    assert_eq!(delivery.code(), Some(7));
    measure_partial(&published, &CONE, 137.5, PI * 100. * 15. / 3., None);
    // Cancelled on the shared job: nothing published, no scratch left.
    let names = entries(d.path());
    let token = ferritecad_kernel::CancelToken::new();
    token.cancel();
    let never: PathBuf = d.path().join("never.fcad");
    let error = ferritecad_jobs::create_document_with_kernel(
        ferritecad_jobs::CreateDocumentRequest::new(
            &never,
            ferritecad_jobs::NewDocument::SketchPartialRevolve {
                profile: FullTurnRevolution::new(CONE.to_vec()).expect("policy"),
                angle: RevolveAngle::new(137.5).expect("angle"),
            },
            "test",
        ),
        ferritecad_occt::OcctKernel::new,
        &OperationContext::default().with_cancel(token),
    )
    .expect_err("cancelled");
    assert_eq!(error.kind(), ErrorKind::Cancellation);
    assert!(!never.exists());
    assert_eq!(entries(d.path()), names, "no scratch left behind");
}

#[test]
fn native_saved_partial_revolve_refuses_coordinate_editing_atomically() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("sector.json");
    let source = d.path().join("sector.fcad");
    request_v2(&input, &BUSHING, angle(90.));
    reply(create(&input, &source).output().expect("create"), 0);
    let before = std::fs::read(&source).expect("source");
    let c = inspect(&source);
    let sketch = c["sketches"][0]["sketch_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let version = c["content_version"].as_str().expect("version").to_owned();
    let ids: Vec<String> = c["revolves"][0]["profile"]["segments"]
        .as_array()
        .expect("segments")
        .iter()
        .map(|s| s["curve_id"].as_str().expect("id").to_owned())
        .collect();
    let request = d.path().join("edit.json");
    write_edit(
        &request,
        &ids,
        &[[5., 0.], [10., 0.], [10., 15.], [5., 15.]],
    );
    let out = d.path().join("edited.fcad");
    let names = entries(d.path());
    let v = edit_reply(
        edit(&source, &sketch, &version, &request, &out)
            .output()
            .expect("edit"),
        2,
    );
    assert_eq!(v["error"]["kind"], "unsupported", "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .expect("message")
            .contains("partial Revolve"),
        "{v}"
    );
    assert_eq!(entries(d.path()), names);
    assert_eq!(std::fs::read(&source).expect("source"), before);
    assert_eq!(c["edit_extrude"]["available"], false);
}

/// A sector document written without a kernel: the layout and the
/// capabilities move with the meaning, a header that disagrees with the
/// payload is refused, and discovery reports the angle.
#[test]
fn partial_revolution_payload_capabilities_and_discovery_without_kernel() {
    let d = tempfile::tempdir().expect("dir");
    for (points, solid) in [(BUSHING.to_vec(), false), (CYLINDER.to_vec(), true)] {
        let path = d.path().join(format!("sector-{solid}.fcad"));
        let (revolve, labels) = write_revolve_document(&path, &points);
        let axis = axis_line(&points).map(|i| labels[i]);
        let sketch = doc_sketch(&path);
        {
            let mut doc = Document::open(&path).expect("open");
            doc.write(|w| {
                w.put_object(
                    revolve,
                    None,
                    2,
                    Some("Revolve1"),
                    &ObjectPayload::Revolve(Revolve {
                        profile: sketch,
                        axis: RevolveAxis::SketchY,
                        extent: RevolveExtent::Partial {
                            degrees: RevolveAngle::new(137.5).expect("angle"),
                        },
                        operation: SolidOperation::NewBody,
                        axis_segment: axis,
                    }),
                )?;
                for side in [CapSide::Start, CapSide::End] {
                    w.put_topology_ref(&TopologyRef {
                        id: StableEntityId::new(),
                        owner: revolve,
                        producer_feature: revolve,
                        expected_kind: EntityKind::Face,
                        output_role: SemanticRole::RevolveCap { side },
                        selection: SelectionRule::Exact,
                        fallback_signature: None,
                    })?;
                }
                Ok(())
            })
            .expect("sector");
            doc.close().expect("close");
        }
        assert_eq!(payload_version(&path), if solid { 4 } else { 3 });
        assert_eq!(capability_rows(&path), expected_capabilities(solid));
        let doc = Document::open_read_only(&path).expect("open");
        let record = doc.object(revolve).expect("row").expect("revolve");
        let ObjectPayload::Revolve(stored) = &record.payload else {
            panic!("a Revolve")
        };
        assert_eq!(
            stored.extent,
            RevolveExtent::Partial {
                degrees: RevolveAngle::new(137.5).expect("angle")
            }
        );
        // The cache key moves with the angle and never equals a full turn's.
        let tolerance = ferritecad_types::Tolerance::default();
        let key = stored.cache_key(tolerance);
        let mut other = stored.clone();
        other.extent = RevolveExtent::Partial {
            degrees: RevolveAngle::new(137.25).expect("angle"),
        };
        assert_ne!(key, other.cache_key(tolerance));
        other.extent = RevolveExtent::FullTurn;
        assert_ne!(key, other.cache_key(tolerance));
        doc.close().expect("close");
        let c = inspect(&path);
        assert_eq!(c["revolves"][0]["extent"], "partial_turn");
        assert_eq!(c["revolves"][0]["angle_deg"], 137.5);
        assert_eq!(c["sketches"][0]["editable"], false);
        // A header claiming the full-turn layout over a sector is refused: it
        // would hide the angle from a build that reads that layout.
        {
            let full_turn_layout = if solid { 2 } else { 1 };
            let raw = rusqlite::Connection::open(&path).expect("SQL");
            let bytes: Vec<u8> = raw
                .query_row(
                    "SELECT payload FROM objects WHERE kind='feature.revolve'",
                    [],
                    |r| r.get(0),
                )
                .expect("payload");
            let mut envelope = ferritecad_document::Envelope::from_bytes(&bytes).expect("envelope");
            envelope.schema_version = full_turn_layout;
            envelope
                .required_capabilities
                .retain(|c| c != "feature.revolve.partial.v1");
            let bytes = envelope.to_bytes().expect("bytes");
            raw.execute(
                "UPDATE objects SET payload=?1,payload_hash=?2,schema_version=?3 \
                 WHERE kind='feature.revolve'",
                rusqlite::params![
                    bytes,
                    ferritecad_types::ContentHash::of_bytes(&bytes)
                        .as_bytes()
                        .as_slice(),
                    full_turn_layout
                ],
            )
            .expect("forge");
        }
        let doc = Document::open_read_only(&path).expect("open");
        let error = doc.objects().expect_err("a full-turn header over a sector");
        assert!(
            error.to_string().contains("does not match what it holds"),
            "{error}"
        );
        doc.close().expect("close");
    }
}

/// The sketch a document written by `write_revolve_document` holds.
pub(super) fn doc_sketch(path: &Path) -> ObjectId {
    let doc = Document::open_read_only(path).expect("open");
    let id = doc
        .objects()
        .expect("objects")
        .iter()
        .find_map(|o| matches!(o.payload, ObjectPayload::Sketch(_)).then_some(o.id))
        .expect("sketch");
    doc.close().expect("close");
    id
}

#[test]
fn occt_without_solver_turns_a_partial_revolve() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("cylinder.json");
    let out = d.path().join("cylinder.fcad");
    request_v2(&input, &CYLINDER, angle(180.));
    reply(create(&input, &out).output().expect("create"), 0);
    let full = PI * 100. * 15.;
    measure_partial(&out, &CYLINDER, 180., full, None);
    let m = mesh(&out, &out.with_extension("stl"));
    check_partial_mesh(&m, &CYLINDER, 180., full);
}
