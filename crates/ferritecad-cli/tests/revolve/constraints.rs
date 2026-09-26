// SPDX-License-Identifier: MIT
//! §27G: dimensional constraints on the saved profile of a Revolve with a
//! bore, added, changed and removed through the existing
//! `edit-sketch-constraints-copy`, and measured after the fact.
//!
//! What the solver finds is read back from a cold rebuild — its degrees of
//! freedom, what it calls redundant and the solved Lines — and the Body is
//! then measured against those Lines alone: the volume by this file's own
//! Pappus integral, every saved face under its own UUID, and on a sector both
//! end faces' planes, outward sides and areas. The stored coordinates are the
//! solver's starting geometry and must never change.
use super::angle::{angle_edit, angle_reply, refs, revolve_payload, target, write_angle};
use super::axis::write_solid_document;
use super::edit::cells;
use super::partial::{
    angle, cap_frame, check_partial_mesh, cross, dot, in_profile, length, pappus, profile_area,
    request_v2, signed_phi,
};
use super::*;
use ferritecad_document::{
    RevolveAngle, SketchConstraint, SketchConstraintRule, SketchPointRef, SketchPointSelector,
};
use ferritecad_kernel::{CancelToken, ProgressSink};
use std::collections::BTreeSet;

const EDIT: &str = "edit-sketch-constraints-copy";

/// A full-turn bushing, off the origin in Y, with fractional radii.
const BUSHING_T: [[f64; 2]; 4] = [[4.25, -1.5], [10.5, -1.5], [10.5, 13.75], [4.25, 13.75]];
/// The same bushing once its bottom Line is dimensioned 7.5 mm from the pin.
const BUSHING_WIDE: [[f64; 2]; 4] = [[4.25, -1.5], [11.75, -1.5], [11.75, 13.75], [4.25, 13.75]];
/// A stepped sector with a bore, a cone between the step and the top, and
/// fractional coordinates shifted along the axis.
const SECTOR_P: [[f64; 2]; 5] = [
    [2.75, -3.25],
    [7.5, -3.25],
    [7.5, 2.125],
    [4.25, 9.5],
    [2.75, 9.5],
];
/// The same sector once its outer wall is dimensioned 7 mm tall.
const SECTOR_TALL: [[f64; 2]; 5] = [
    [2.75, -3.25],
    [7.5, -3.25],
    [7.5, 3.75],
    [4.25, 9.5],
    [2.75, 9.5],
];
const SECTOR_DEG: f64 = 137.5;

fn solver() -> bool {
    if ferritecad_occt::is_available() && cfg!(feature = "planegcs") {
        return true;
    }
    assert_ne!(
        std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
        Ok("1")
    );
    eprintln!("skipped: solved Revolve constraints require OCCT and PlaneGCS");
    false
}

fn constrain(source: &Path, catalog: &Value, request: &Path, out: &Path) -> Command {
    let mut c = cli();
    c.arg(EDIT)
        .arg(source)
        .arg("--sketch")
        .arg(
            catalog["sketches"][0]["sketch_id"]
                .as_str()
                .expect("sketch"),
        )
        .arg("--expect-version")
        .arg(catalog["content_version"].as_str().expect("version"))
        .arg("--request")
        .arg(request)
        .arg("-o")
        .arg(out)
        .arg("--json");
    c
}
fn edit_reply(out: Output, code: i32) -> Value {
    assert_eq!(out.status.code(), Some(code), "{out:?}");
    assert_eq!(out.stdout.iter().filter(|&&b| b == b'\n').count(), 1);
    let v: Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["operation"], EDIT);
    assert_eq!(v["ok"], code == 0, "{v}");
    v
}
/// Publish one request and return its result.
fn publish(source: &Path, request: &Path, edits: &Value, out: &Path) -> Value {
    let catalog = inspect(source);
    write(request, edits);
    edit_reply(
        constrain(source, &catalog, request, out)
            .output()
            .expect("constrain"),
        0,
    )["result"]
        .clone()
}
/// Refuse one request: nothing appears, the source is byte-identical.
fn refuse(source: &Path, request: &Path, edits: &Value, out: &Path) -> Value {
    let catalog = inspect(source);
    let before = std::fs::read(source).expect("source");
    let directory = entries(out.parent().expect("dir"));
    write(request, edits);
    let v = edit_reply(
        constrain(source, &catalog, request, out)
            .output()
            .expect("refusal"),
        2,
    );
    assert!(!out.exists(), "{v}");
    assert_eq!(entries(out.parent().expect("dir")), directory, "no scratch");
    assert_eq!(std::fs::read(source).expect("source"), before);
    v["error"].clone()
}

fn curve(catalog: &Value, i: usize) -> Value {
    catalog["sketches"][0]["constraint_edit"]["curves"][i]["curve_id"].clone()
}
fn rule(catalog: &Value, i: usize, rule: &str) -> Value {
    json!({"curve_id": curve(catalog, i), "rule": rule})
}
fn distance(catalog: &Value, i: usize, mm: f64) -> Value {
    json!({"curve_id": curve(catalog, i), "rule": "distance", "distance_mm": mm})
}
fn fixed(catalog: &Value, i: usize, at: &str, x: f64, y: f64) -> Value {
    json!({"curve_id": curve(catalog, i), "rule": "fixed", "at": at, "x_mm": x, "y_mm": y})
}
fn pair(catalog: &Value, kind: &str, i: usize, j: usize) -> Value {
    json!({"rule": kind, "a_curve_id": curve(catalog, i), "b_curve_id": curve(catalog, j)})
}
fn adds(add: Vec<Value>) -> Value {
    json!({"request_version": 1, "remove": [], "add": add})
}
/// The one stored constraint of `kind` on Line `i`, by its UUID.
fn stored_id(catalog: &Value, kind: &str, i: usize) -> Value {
    let id = curve(catalog, i);
    catalog["sketches"][0]["constraint_edit"]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .find(|c| {
            c["rule"]["kind"] == kind
                && (c["rule"]["a"]["curve_id"] == id || c["rule"]["point"]["curve_id"] == id)
        })
        .unwrap_or_else(|| panic!("no {kind} on Line {i}"))["constraint_id"]
        .clone()
}
fn constraint_ids(catalog: &Value) -> Vec<Value> {
    catalog["sketches"][0]["constraint_edit"]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .map(|c| c["constraint_id"].clone())
        .collect()
}

fn sketch_of(path: &Path) -> (ObjectId, Sketch) {
    let d = Document::open_read_only(path).expect("open");
    let found = d
        .objects()
        .expect("objects")
        .into_iter()
        .find_map(|o| match o.payload {
            ObjectPayload::Sketch(s) => Some((o.id, s)),
            _ => None,
        })
        .expect("sketch");
    d.close().expect("close");
    found
}

/// The §27G allowlist, cell by cell. In `objects`, only the Sketch row's
/// `payload`, `payload_hash` and `schema_version`; in `capabilities`, only the
/// `sketch.constraints.v1` row, which may appear as required or turn
/// required. Every other row and cell of every table — the Revolve row, the
/// Body, the plane, the dependencies, every name and `meta` — is equal.
fn check_constraint_cells(source: &Path, copy: &Path, sketch: ObjectId) {
    let id = format!("id=Blob({:?})", sketch.to_bytes().to_vec());
    let (a, b) = (cells(source), cells(copy));
    assert_eq!(
        a.keys().collect::<Vec<_>>(),
        b.keys().collect::<Vec<_>>(),
        "tables"
    );
    let capability = |r: &Vec<String>| {
        r.contains(&format!(
            "name=Text({:?})",
            ferritecad_document::SKETCH_CONSTRAINTS_CAPABILITY
        ))
    };
    for (table, old) in &a {
        let new = &b[table];
        match table.as_str() {
            "objects" => {
                let strip = |rows: &Vec<Vec<String>>| -> Vec<Vec<String>> {
                    let mut kept: Vec<Vec<String>> = rows
                        .iter()
                        .map(|r| {
                            r.iter()
                                .filter(|c| {
                                    !(r.contains(&id)
                                        && (c.starts_with("payload=")
                                            || c.starts_with("payload_hash=")
                                            || c.starts_with("schema_version=")))
                                })
                                .cloned()
                                .collect()
                        })
                        .collect();
                    kept.sort();
                    kept
                };
                assert_eq!(
                    strip(old),
                    strip(new),
                    "objects: a cell outside the allowlist"
                );
                let changed: Vec<_> = old.iter().filter(|r| !new.contains(r)).collect();
                assert!(
                    changed.iter().all(|r| r.contains(&id)),
                    "only the Sketch row"
                );
            }
            "capabilities" => {
                let others = |rows: &Vec<Vec<String>>| -> Vec<Vec<String>> {
                    rows.iter().filter(|r| !capability(r)).cloned().collect()
                };
                assert_eq!(others(old), others(new), "unrelated capabilities");
                let row: Vec<_> = new.iter().filter(|r| capability(r)).collect();
                assert_eq!(row.len(), 1, "{new:?}");
                assert!(
                    row[0].contains(&"required=Integer(1)".to_owned()),
                    "{row:?}"
                );
            }
            _ => assert_eq!(old, new, "{table} changed"),
        }
    }
}

/// What a cold or cached rebuild says about one saved constrained Revolve.
struct Solved {
    dof: usize,
    redundant: Vec<StableEntityId>,
    /// The solved Lines in stored order: `(curve UUID, start, end)`.
    lines: Vec<(StableEntityId, [f64; 2], [f64; 2])>,
    volume: f64,
}
impl Solved {
    fn starts(&self) -> Vec<[f64; 2]> {
        self.lines.iter().map(|(_, a, _)| *a).collect()
    }
}
fn near(a: [f64; 2], b: [f64; 2], tolerance: f64) -> bool {
    (a[0] - b[0]).abs() <= tolerance && (a[1] - b[1]).abs() <= tolerance
}
fn assert_solved(solved: &Solved, expected: &[[f64; 2]]) {
    let starts = solved.starts();
    assert_eq!(starts.len(), expected.len());
    for (s, e) in starts.iter().zip(expected) {
        assert!(near(*s, *e, 1e-7), "solved {starts:?} != {expected:?}");
    }
}

/// The kernel's account of a constrained Revolve, measured against its
/// solved Lines only. `degrees` is `None` for a full turn. `cache` is a store
/// path shared across copies, and the outcomes this rebuild must report.
fn measure_solved(
    path: &Path,
    degrees: Option<f64>,
    cache: Option<(&Path, &[CacheOutcome])>,
) -> Solved {
    let d = Document::open_read_only(path).expect("reopen");
    assert!(d.validate().expect("validate").is_ok());
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let context = OperationContext::default();
    let built = if let Some((store, expected)) = cache {
        let mut store = CacheStore::open(
            store,
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
    let (sketch_id, stored) = objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Sketch(s) => Some((o.id, s.clone())),
            _ => None,
        })
        .expect("sketch");
    let (revolve, body) = objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Body(b) => Some((b.tip_feature.expect("tip"), o.id)),
            _ => None,
        })
        .expect("body");
    let report = built.solve_report(sketch_id).expect("one solve report");
    let picture = built
        .sketch_presentation(sketch_id)
        .expect("solved drawing");
    let lines: Vec<_> = picture
        .curves()
        .iter()
        .zip(&stored.curves)
        .map(|(c, s)| {
            assert_eq!(c.id(), s.id, "solved in stored order");
            let SketchGeometry::Line { start, end } = *c.geometry() else {
                panic!("a Line")
            };
            (c.id(), [start.x, start.y], [end.x, end.y])
        })
        .collect();
    assert_eq!(lines.len(), stored.curves.len());
    for (i, (_, _, end)) in lines.iter().enumerate() {
        let next = lines[(i + 1) % lines.len()].1;
        assert!(near(*end, next, 1e-7), "joint {i}: {end:?} != {next:?}");
    }
    let points: Vec<[f64; 2]> = lines.iter().map(|(_, a, _)| *a).collect();
    let n = points.len();
    assert_eq!(built.shape_count(), 1);
    let shape = built.shape(body).expect("Body");
    assert_eq!(built.shape(revolve), Some(shape));
    let (faces, volume) = kernel.shape_stats(shape).expect("stats");
    let turn = degrees.unwrap_or(360.);
    let expected = pappus(&points) * turn / 360.;
    assert!(
        (volume - expected).abs() < 1e-8 * expected,
        "{volume} != {expected} for {points:?} through {turn}°"
    );
    let caps = usize::from(degrees.is_some()) * 2;
    assert_eq!(
        faces as usize,
        n + caps,
        "one face per Line, and two end faces"
    );
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
    let by_id: BTreeMap<_, _> = lines.iter().map(|(id, a, b)| (*id, (*a, *b))).collect();
    let refs = d.topology_refs().expect("refs");
    assert_eq!(refs.len(), n + caps, "one saved name per face");
    let mut named = BTreeSet::new();
    let mut sides = BTreeSet::new();
    let radius = |p: &[f64; 3]| p[0].hypot(p[2]);
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
                assert_eq!(
                    reference.selection,
                    SelectionRule::AllDerivedFrom {
                        ancestor: profile_segment
                    }
                );
                let (a, b) = by_id[&profile_segment];
                if (b[1] - a[1]).abs() < 1e-9 {
                    assert_eq!(surface, FaceSurface::Plane, "{profile_segment}");
                    assert!(vertices.iter().all(|p| (p[1] - a[1]).abs() < 1e-6));
                } else if (b[0] - a[0]).abs() < 1e-9 {
                    let FaceSurface::Cylinder { radius: r } = surface else {
                        panic!("{profile_segment}: {surface:?} is not a cylinder")
                    };
                    assert!((r - a[0]).abs() < 1e-9, "{r} != {}", a[0]);
                    assert!(vertices.iter().all(|p| (radius(p) - a[0]).abs() < 1e-4));
                } else {
                    assert_eq!(surface, FaceSurface::Cone, "{profile_segment}");
                    for p in &vertices {
                        let s = (p[1] - a[1]) / (b[1] - a[1]);
                        assert!((radius(p) - (a[0] + s * (b[0] - a[0]))).abs() < LINEAR_MM);
                    }
                }
                if surface != FaceSurface::Plane {
                    let (origin, direction) = kernel.surface_axis(*face).expect("axis");
                    assert!(origin[0].abs() < 1e-9 && origin[2].abs() < 1e-9);
                    assert!((direction[1].abs() - 1.).abs() < 1e-12);
                }
                match degrees {
                    // Turned from the start plane exactly to the end plane.
                    Some(degrees) => {
                        let angles: Vec<f64> = vertices
                            .iter()
                            .filter(|p| radius(p) > 1e-3)
                            .map(signed_phi)
                            .collect();
                        let lo = angles.iter().copied().fold(f64::INFINITY, f64::min);
                        let hi = angles.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                        assert!(lo.abs() < 1e-3, "starts at {lo}°");
                        assert!((hi - degrees).abs() < 1e-3, "ends at {hi}°");
                    }
                    // All the way round.
                    None => {
                        for (sx, sz) in [(1., 1.), (-1., 1.), (-1., -1.), (1., -1.)] {
                            assert!(
                                vertices
                                    .iter()
                                    .any(|p| p[0] * sx > 1e-3 && p[2] * sz > 1e-3)
                            );
                        }
                    }
                }
            }
            SemanticRole::RevolveCap { side } => {
                let degrees = degrees.expect("a full turn has no end faces");
                assert_eq!(reference.selection, SelectionRule::Exact);
                assert_eq!(reference.producer_feature, revolve);
                assert_eq!(surface, FaceSurface::Plane);
                let (outward, radial) = cap_frame(side, degrees);
                for p in &vertices {
                    assert!(
                        dot(*p, outward).abs() < 1e-5,
                        "{p:?} off the {side:?} plane"
                    );
                    assert!(
                        in_profile(&points, dot(*p, radial), p[1], 1e-5),
                        "{p:?} is outside the solved profile on the {side:?} face"
                    );
                }
                let sum = t
                    .iter()
                    .map(cross)
                    .fold([0.; 3], |s, c| [s[0] + c[0], s[1] + c[1], s[2] + c[2]]);
                let covered: f64 = t.iter().map(|x| length(cross(x)) / 2.).sum();
                let area = profile_area(&points);
                assert!(
                    dot(sum, outward) / length(sum) > 1. - 1e-9,
                    "the {side:?} face does not face out"
                );
                assert!(
                    (covered - area).abs() < 1e-5 * area,
                    "the {side:?} face covers {covered} of {area}"
                );
                assert!(sides.insert(side));
            }
            ref other => panic!("unexpected role {other:?}"),
        }
    }
    assert_eq!(named.len(), faces as usize, "every face has one name");
    assert_eq!(sides.len(), caps);
    let solved = Solved {
        dof: report.degrees_of_freedom(),
        redundant: report.redundant().to_vec(),
        lines,
        volume,
    };
    built.release_all(&mut kernel);
    d.close().expect("close");
    assert_eq!(kernel.live_shape_count(), 0);
    solved
}

/// STL, independently integrated against the solved profile, and FBX; the
/// pair kept for the pinned reader when the gate asks for artifacts.
fn exports(path: &Path, solved: &Solved, degrees: Option<f64>, artifact: &str) {
    let points = solved.starts();
    let m = mesh(path, &path.with_extension("stl"));
    match degrees {
        Some(degrees) => check_partial_mesh(&m, &points, degrees, pappus(&points)),
        None => check_mesh(&m, &points),
    }
    let fbx = path.with_extension("fbx");
    let r = cli()
        .arg("export-fbx")
        .arg(path)
        .arg("-o")
        .arg(&fbx)
        .arg("--json")
        .output()
        .expect("FBX");
    assert!(r.status.success(), "{r:?}");
    let v: Value = serde_json::from_slice(&r.stdout).expect("JSON");
    assert_eq!(v["result"]["complete"], true, "{v}");
    if let Some(dir) = std::env::var_os("FCAD_REVOLVE_CONSTRAINT_ARTIFACTS") {
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).expect("artifacts");
        // Both at the default tessellation, the only one export-fbx has, so
        // the CI join compares one mesh.
        for (op, extension) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
            let r = cli()
                .arg(op)
                .arg(path)
                .arg("-o")
                .arg(dir.join(format!("{artifact}.{extension}")))
                .arg("--json")
                .output()
                .expect("artifact");
            assert!(r.status.success(), "{r:?}");
        }
    }
}

fn create_full(root: &Path, points: &[[f64; 2]], name: &str) -> PathBuf {
    let input = root.join(format!("{name}.json"));
    request(&input, points);
    let out = root.join(format!("{name}.fcad"));
    reply(create(&input, &out).output().expect("create"), 0);
    out
}
fn create_sector(root: &Path, points: &[[f64; 2]], degrees: f64, name: &str) -> PathBuf {
    let input = root.join(format!("{name}.json"));
    request_v2(&input, points, angle(degrees));
    let out = root.join(format!("{name}.fcad"));
    reply(create(&input, &out).output().expect("create"), 0);
    out
}

/// A full-turn bushing: dimensioned to a rigid profile equal to its stored
/// coordinates, then one dimension replaced so the Body grows, then every
/// user constraint removed again. Each copy is measured cold and through a
/// cache it shares with the one before, compared cell by cell with its
/// source, and exported. Conflicts, redundancy and solved profiles outside
/// the class are the real solver's and the shared policy's answers.
#[test]
fn native_bushing_constraints_dimension_replace_remove_cache_and_exports() {
    if !solver() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let root = d.path();
    let source = create_full(root, &BUSHING_T, "bushing");
    let request = root.join("constraints.json");
    let (sketch, stored) = sketch_of(&source);
    let catalog = inspect(&source);
    let row = &catalog["sketches"][0];
    assert_eq!(row["constraint_edit"]["available"], true, "{row}");
    assert_eq!(
        row["constraint_edit"]["profile_feature"]["kind"],
        "full_turn_revolve"
    );
    let source_refs = refs(&source);
    let source_bytes = std::fs::read(&source).expect("source");

    // Rigid: H/V on all four Lines, the inner bottom corner pinned where it
    // is stored, both sizes as stored. Nothing moves; nothing is free.
    let rigid = root.join("rigid.fcad");
    let published = publish(
        &source,
        &request,
        &adds(vec![
            rule(&catalog, 0, "horizontal"),
            rule(&catalog, 1, "vertical"),
            rule(&catalog, 2, "horizontal"),
            rule(&catalog, 3, "vertical"),
            fixed(&catalog, 0, "start", 4.25, -1.5),
            distance(&catalog, 0, 6.25),
            distance(&catalog, 1, 15.25),
        ]),
        &rigid,
    );
    assert_eq!(published["sketch_id"], sketch.to_string());
    assert_eq!(
        published["added_constraints"]
            .as_array()
            .expect("added")
            .len(),
        4 + 7,
        "four closure links, then the seven asked for"
    );
    assert_eq!(published["removed_constraint_ids"], json!([]));
    assert_eq!(published["solve"]["degrees_of_freedom"], 0);
    assert_eq!(published["solve"]["redundant_constraint_ids"], json!([]));
    check_constraint_cells(&source, &rigid, sketch);
    assert_eq!(sketch_of(&rigid).1.curves, stored.curves, "stored inputs");
    assert_eq!(refs(&rigid), source_refs);
    assert_eq!(revolve_payload(&rigid), revolve_payload(&source));
    let cache = root.join("shared.fcad-cache");
    let solved = measure_solved(&rigid, None, Some((&cache, &[CacheOutcome::Miss])));
    assert_eq!((solved.dof, solved.redundant.len()), (0, 0));
    assert_solved(&solved, &BUSHING_T);
    let exact = |outer: f64, inner: f64, height: f64| PI * (outer * outer - inner * inner) * height;
    assert!((solved.volume - exact(10.5, 4.25, 15.25)).abs() < 1e-7 * solved.volume);
    measure_solved(&rigid, None, Some((&cache, &[CacheOutcome::Hit])));

    // A changed dimension moves the Body: the bottom Line's length is
    // replaced — the exact old UUID removed, one new one minted — and the
    // outer wall follows it out to 11.75 mm.
    let catalog = inspect(&rigid);
    let old_width = stored_id(&catalog, "distance", 0);
    let wide = root.join("wide.fcad");
    let replaced = publish(
        &rigid,
        &request,
        &json!({"request_version":1,"remove":[old_width.clone()],"add":[distance(&catalog, 0, 7.5)]}),
        &wide,
    );
    assert_eq!(replaced["removed_constraint_ids"], json!([old_width]));
    let [new_width] = replaced["added_constraints"]
        .as_array()
        .expect("added")
        .as_slice()
    else {
        panic!("one new constraint: {replaced}")
    };
    assert_eq!(new_width["rule"]["distance"], 7.5);
    assert_eq!(replaced["solve"]["degrees_of_freedom"], 0);
    let mut kept = constraint_ids(&catalog);
    kept.retain(|id| *id != old_width);
    kept.push(new_width["constraint_id"].clone());
    assert_eq!(
        constraint_ids(&inspect(&wide)),
        kept,
        "every other UUID is kept"
    );
    check_constraint_cells(&rigid, &wide, sketch);
    assert_eq!(
        sketch_of(&wide).1.curves,
        stored.curves,
        "stored inputs unchanged"
    );
    assert_eq!(refs(&wide), source_refs);
    // A different solution is a different key: Miss, then Hit, through the
    // store the rigid copy filled.
    let grown = measure_solved(&wide, None, Some((&cache, &[CacheOutcome::Miss])));
    measure_solved(&wide, None, Some((&cache, &[CacheOutcome::Hit])));
    let cold = measure_solved(&wide, None, None);
    assert_eq!(cold.lines, grown.lines, "cold and cached solve alike");
    assert_eq!(grown.dof, 0);
    assert_solved(&grown, &BUSHING_WIDE);
    assert!((grown.volume - exact(11.75, 4.25, 15.25)).abs() < 1e-7 * grown.volume);
    assert!(grown.volume > solved.volume + 1.);
    exports(&wide, &grown, None, "revolve-constraints-bushing");

    // Redundancy is the solver's diagnosis: a rectangle's opposite sides are
    // already equal, so saying so again changes nothing it can solve.
    let catalog = inspect(&wide);
    let redundant = root.join("redundant.fcad");
    let said = publish(
        &wide,
        &request,
        &adds(vec![pair(&catalog, "equal_length", 0, 2)]),
        &redundant,
    );
    let equality = said["added_constraints"][0]["constraint_id"].clone();
    let named = said["solve"]["redundant_constraint_ids"]
        .as_array()
        .expect("redundant")
        .clone();
    assert!(!named.is_empty(), "{said}");
    let known: Vec<Value> = constraint_ids(&inspect(&redundant));
    assert!(named.iter().all(|id| known.contains(id)), "{named:?}");
    // Measured on the CI solver: it names the equality that repeats what
    // the rectangle already says.
    assert!(named.contains(&equality), "{named:?} does not name {equality}");
    assert_eq!(said["solve"]["degrees_of_freedom"], 0);
    assert_solved(&measure_solved(&redundant, None, None), &BUSHING_WIDE);

    // Refused before publication, every source untouched.
    let refused = root.join("refused.fcad");
    let conflict = refuse(
        &wide,
        &request,
        &adds(vec![pair(&catalog, "equal_length", 0, 1)]),
        &refused,
    );
    assert_eq!(conflict["kind"], "constraint", "{conflict}");
    let named = conflict["constraint_conflict"]["constraints"]
        .as_array()
        .expect("typed conflict");
    assert!(named.iter().any(|c| c["rule"]["kind"] == "equal_length"));
    assert!(
        named
            .iter()
            .all(|c| known.contains(&c["constraint_id"]) || c["rule"]["kind"] == "equal_length")
    );
    let pin = stored_id(&catalog, "fixed", 0);
    for (why, x, wanted) in [
        ("crosses the axis", -1.0, "axis"),
        ("reaches the axis", 0.0, "axis"),
    ] {
        let error = refuse(
            &wide,
            &request,
            &json!({"request_version":1,"remove":[pin.clone()],"add":[fixed(&catalog, 0, "start", x, -1.5)]}),
            &refused,
        );
        assert!(
            error["message"].as_str().expect("message").contains(wanted),
            "{why}: {error}"
        );
        assert!(error.get("constraint_conflict").is_none(), "{why}");
    }
    // Only closure holds the saved bushing: one pinned corner moves alone.
    let catalog = inspect(&source);
    // A collapsed Line is refused by the shared profile reading — measured
    // on the CI solver as "a line segment needs two distinct endpoints" —
    // before the policy's own zero-length check can see it.
    for (why, x, y, wanted) in [
        ("self-intersects", 2.0, 13.0, &["intersect"][..]),
        (
            "collapses a Line",
            4.25,
            -1.5,
            &["zero-length", "left over", "two distinct endpoints"][..],
        ),
    ] {
        let error = refuse(
            &source,
            &request,
            &adds(vec![fixed(&catalog, 0, "end", x, y)]),
            &refused,
        );
        let message = error["message"].as_str().expect("message");
        assert!(wanted.iter().any(|w| message.contains(w)), "{why}: {error}");
    }

    // Removing every user constraint keeps the closure, the stored inputs
    // and the requirement; the profile is free again but still solved, and
    // coordinate edits stay refused by the shared policy.
    let catalog = inspect(&wide);
    let user: Vec<Value> = catalog["sketches"][0]["constraint_edit"]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .filter(|c| c["rule"]["kind"] != "coincident")
        .map(|c| c["constraint_id"].clone())
        .collect();
    assert_eq!(user.len(), 7);
    let bare = root.join("bare.fcad");
    let removed = publish(
        &wide,
        &request,
        &json!({"request_version":1,"remove":user,"add":[]}),
        &bare,
    );
    assert_eq!(removed["added_constraints"], json!([]));
    assert_eq!(removed["solve"]["degrees_of_freedom"], 8);
    let row = inspect(&bare)["sketches"][0].clone();
    let left = row["constraint_edit"]["constraints"]
        .as_array()
        .expect("closure");
    assert_eq!(left.len(), 4);
    assert!(left.iter().all(|c| c["rule"]["kind"] == "coincident"));
    assert_eq!(row["editable"], false);
    assert!(
        row["refusal"]
            .as_str()
            .expect("refusal")
            .contains("unconstrained"),
        "{row}"
    );
    assert_eq!(row["constraint_edit"]["available"], true);
    check_constraint_cells(&source, &bare, sketch);
    let free = measure_solved(&bare, None, None);
    assert_eq!(free.dof, 8);
    assert_solved(&free, &BUSHING_T);
    assert!((free.volume - exact(10.5, 4.25, 15.25)).abs() < 1e-7 * free.volume);

    // Delivery and the shared guards on this class.
    let catalog = inspect(&rigid);
    write(&request, &adds(vec![pair(&catalog, "equal_length", 0, 2)]));
    let lost = root.join("lost.fcad");
    let mut command = constrain(&rigid, &catalog, &request, &lost);
    command.stdout(pipe::closed_pipe());
    assert_eq!(
        command.output().expect("lost report").status.code(),
        Some(7)
    );
    assert!(
        Document::open_read_only(&lost).is_ok(),
        "published despite the lost report"
    );
    let stale = edit_reply(
        constrain(&wide, &catalog, &request, &root.join("stale.fcad"))
            .output()
            .expect("stale"),
        2,
    );
    assert!(
        stale["error"]["message"]
            .as_str()
            .expect("m")
            .contains("changed"),
        "{stale}"
    );
    let alias = edit_reply(
        constrain(&rigid, &catalog, &request, &rigid)
            .output()
            .expect("alias"),
        2,
    );
    assert!(
        alias["error"]["message"]
            .as_str()
            .expect("m")
            .contains("different files")
    );
    let taken = edit_reply(
        constrain(&rigid, &catalog, &request, &wide)
            .output()
            .expect("taken"),
        2,
    );
    assert!(
        taken["error"]["message"]
            .as_str()
            .expect("m")
            .contains("exists")
    );
    assert!(!root.join("stale.fcad").exists());

    // The same job, cancelled after the solved copy was checked and before
    // publication, and raced by another writer for the destination.
    let expected = {
        let doc = Document::open_read_only(&rigid).expect("open");
        let version = ferritecad_document::DocumentVersion {
            document_id: doc.meta().document_id,
            content: doc.content_version().expect("version"),
        };
        doc.close().expect("close");
        version
    };
    assert_eq!(
        expected.content.to_string(),
        catalog["content_version"].as_str().expect("version")
    );
    let job = |out: &str| ferritecad_jobs::EditSketchConstraintsRequest {
        source: rigid.clone(),
        expected,
        sketch,
        edits: ferritecad_document::SketchConstraintEdits {
            remove: vec![],
            add: vec![ferritecad_document::AddSketchConstraint::Line(
                ferritecad_document::AddLineConstraint::EqualLength {
                    a: stored.curves[0].id,
                    b: stored.curves[2].id,
                },
            )],
        },
        destination: root.join(out),
    };
    let names = entries(root);
    let token = CancelToken::new();
    let stop = token.clone();
    let context = OperationContext::default()
        .with_cancel(token)
        .with_progress(ProgressSink::new(move |f| {
            if f >= 0.9 {
                stop.cancel();
            }
        }));
    let error = ferritecad_jobs::edit_sketch_constraints_copy(
        &job("cancelled.fcad"),
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &context,
    )
    .expect_err("cancelled");
    assert_eq!(error.kind(), ErrorKind::Cancellation, "{error}");
    assert_eq!(entries(root), names, "no copy and no scratch");
    let raced = root.join("raced.fcad");
    let path = raced.clone();
    let context = OperationContext::default().with_progress(ProgressSink::new(move |f| {
        if f >= 0.95 && !path.exists() {
            std::fs::write(&path, b"theirs").expect("race");
        }
    }));
    let error = ferritecad_jobs::edit_sketch_constraints_copy(
        &job("raced.fcad"),
        &mut ferritecad_occt::OcctKernel::new().expect("kernel"),
        &context,
    )
    .expect_err("taken during the job");
    assert!(error.to_string().contains("exists"), "{error}");
    assert_eq!(std::fs::read(&raced).expect("theirs"), b"theirs");
    std::fs::remove_file(&raced).expect("clean up");
    assert_eq!(entries(root), names, "no scratch left behind");
    assert_eq!(std::fs::read(&source).expect("source"), source_bytes);
}

/// A fractional sector with a bore: dimensioned rigid, then its outer wall
/// made taller, which moves the cone above it; both end faces keep their
/// names and follow the solved profile. The angle is then edited with the
/// constraints kept, and the last user constraint removed.
#[test]
fn native_sector_constraints_measure_caps_names_angle_edit_and_exports() {
    if !solver() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let root = d.path();
    let source = create_sector(root, &SECTOR_P, SECTOR_DEG, "sector");
    let request = root.join("constraints.json");
    let (sketch, stored) = sketch_of(&source);
    let catalog = inspect(&source);
    let feature = &catalog["sketches"][0]["constraint_edit"]["profile_feature"];
    assert_eq!(feature["kind"], "partial_turn_revolve", "{feature}");
    assert_eq!(feature["angle_deg"], SECTOR_DEG);
    let source_refs = refs(&source);
    assert_eq!(source_refs.len(), 5 + 2);

    let rigid = root.join("rigid.fcad");
    let published = publish(
        &source,
        &request,
        &adds(vec![
            rule(&catalog, 0, "horizontal"),
            rule(&catalog, 1, "vertical"),
            rule(&catalog, 3, "horizontal"),
            rule(&catalog, 4, "vertical"),
            fixed(&catalog, 0, "start", 2.75, -3.25),
            distance(&catalog, 0, 4.75),
            distance(&catalog, 1, 5.375),
            distance(&catalog, 3, 1.5),
            distance(&catalog, 4, 12.75),
        ]),
        &rigid,
    );
    assert_eq!(published["solve"]["degrees_of_freedom"], 0);
    assert_eq!(published["solve"]["redundant_constraint_ids"], json!([]));
    check_constraint_cells(&source, &rigid, sketch);
    assert_eq!(refs(&rigid), source_refs, "both caps and every face name");
    assert_eq!(revolve_payload(&rigid), revolve_payload(&source));
    let cache = root.join("shared.fcad-cache");
    let solved = measure_solved(
        &rigid,
        Some(SECTOR_DEG),
        Some((&cache, &[CacheOutcome::Miss])),
    );
    assert_solved(&solved, &SECTOR_P);

    let catalog = inspect(&rigid);
    let old_wall = stored_id(&catalog, "distance", 1);
    let tall = root.join("tall.fcad");
    let replaced = publish(
        &rigid,
        &request,
        &json!({"request_version":1,"remove":[old_wall.clone()],"add":[distance(&catalog, 1, 7.0)]}),
        &tall,
    );
    assert_eq!(replaced["removed_constraint_ids"], json!([old_wall]));
    assert_eq!(replaced["solve"]["degrees_of_freedom"], 0);
    check_constraint_cells(&rigid, &tall, sketch);
    assert_eq!(
        sketch_of(&tall).1.curves,
        stored.curves,
        "stored inputs unchanged"
    );
    assert_eq!(refs(&tall), source_refs);
    assert_eq!(
        revolve_payload(&tall),
        revolve_payload(&source),
        "angle kept"
    );
    let grown = measure_solved(
        &tall,
        Some(SECTOR_DEG),
        Some((&cache, &[CacheOutcome::Miss])),
    );
    measure_solved(
        &tall,
        Some(SECTOR_DEG),
        Some((&cache, &[CacheOutcome::Hit])),
    );
    assert_solved(&grown, &SECTOR_TALL);
    let exact = pappus(&SECTOR_TALL) * SECTOR_DEG / 360.;
    assert!((grown.volume - exact).abs() < 1e-7 * exact);
    assert!(grown.volume > solved.volume);
    exports(
        &tall,
        &grown,
        Some(SECTOR_DEG),
        "revolve-constraints-sector",
    );

    // The angle edit writes only the Revolve row: the Sketch, constraints
    // included, is byte-identical, and the solved profile turns further.
    let (revolve, version, discovered) = target(&tall);
    assert_eq!(discovered["angle_edit"]["available"], true, "{discovered}");
    assert_eq!(discovered["profile"]["available"], true, "{discovered}");
    let angle_request = root.join("angle.json");
    write_angle(&angle_request, 212.25);
    let turned = root.join("turned.fcad");
    angle_reply(
        angle_edit(&tall, &revolve, &version, &angle_request, &turned)
            .output()
            .expect("angle"),
        0,
    );
    assert_eq!(
        sketch_of(&turned).1,
        sketch_of(&tall).1,
        "the Sketch is kept"
    );
    assert_eq!(refs(&turned), source_refs);
    let wider = measure_solved(&turned, Some(212.25), None);
    assert_solved(&wider, &SECTOR_TALL);
    assert_eq!(wider.dof, 0);

    // Conflicting sizes are the solver's refusal, with real UUIDs.
    let catalog = inspect(&tall);
    let refused = root.join("refused.fcad");
    let conflict = refuse(
        &tall,
        &request,
        &adds(vec![pair(&catalog, "equal_length", 0, 3)]),
        &refused,
    );
    assert_eq!(conflict["kind"], "constraint", "{conflict}");
    assert!(
        conflict["constraint_conflict"]["constraints"]
            .as_array()
            .expect("conflict")
            .iter()
            .any(|c| c["rule"]["kind"] == "equal_length")
    );

    // The last removal of the new wall height frees one dimension; the
    // solver starts from the stored inputs, which satisfy what remains.
    let wall = stored_id(&catalog, "distance", 1);
    let free = root.join("free.fcad");
    let removed = publish(
        &tall,
        &request,
        &json!({"request_version":1,"remove":[wall],"add":[]}),
        &free,
    );
    assert_eq!(removed["solve"]["degrees_of_freedom"], 1);
    let loose = measure_solved(&free, Some(SECTOR_DEG), None);
    assert_eq!(loose.dof, 1);
    assert_solved(&loose, &SECTOR_P);
    assert!((loose.volume - solved.volume).abs() < 1e-7 * solved.volume);
}

/// Discovery, request shapes, the writer and every refusal a build reaches
/// without a solver — in any build, stub or native.
#[test]
fn revolve_constraint_discovery_writer_and_refusals_without_solver() {
    let d = tempfile::tempdir().expect("dir");
    let root = d.path();
    let full = root.join("full.fcad");
    let (_, labels) = write_revolve_document(&full, &BUSHING);
    let sector = root.join("sector.fcad");
    stub_sector(&sector, SECTOR_DEG);
    for (path, kind) in [
        (&full, "full_turn_revolve"),
        (&sector, "partial_turn_revolve"),
    ] {
        let c = inspect(path);
        let row = &c["sketches"][0];
        let edit = &row["constraint_edit"];
        assert_eq!(edit["available"], true, "{row}");
        assert_eq!(edit["refusal"], Value::Null);
        assert_eq!(edit["constraints"], json!([]));
        assert_eq!(edit["circles"], json!([]));
        assert_eq!(edit["curves"].as_array().expect("curves").len(), 4);
        assert_eq!(edit["profile_feature"]["kind"], kind, "{edit}");
        assert_eq!(edit["profile_feature"], row["profile_feature"]);
        assert!(
            edit["profile_feature"].get("height_mm").is_none(),
            "no height"
        );
    }
    assert_eq!(
        inspect(&sector)["sketches"][0]["constraint_edit"]["profile_feature"]["angle_deg"],
        SECTOR_DEG
    );

    // Axis-closed profiles, full and partial, are refused with the reason.
    let solid = root.join("solid.fcad");
    write_solid_document(
        &solid,
        &[[0., 0.], [10., 0.], [10., 15.], [0., 15.]],
        Some(3),
    );
    let solid_sector = root.join("solid-sector.fcad");
    write_solid_document(
        &solid_sector,
        &[[0., 0.], [10., 0.], [10., 15.], [0., 15.]],
        Some(3),
    );
    rewrite_revolve(&solid_sector, |r| {
        r.extent = RevolveExtent::Partial {
            degrees: RevolveAngle::new(90.).expect("angle"),
        }
    });
    for path in [&solid, &solid_sector] {
        let edit = inspect(path)["sketches"][0]["constraint_edit"].clone();
        assert_eq!(edit["available"], false, "{edit}");
        assert!(
            edit["refusal"]
                .as_str()
                .expect("refusal")
                .contains("closed on the axis along Line"),
            "{edit}"
        );
        assert_eq!(edit["curves"], Value::Null);
        assert_eq!(edit["profile_feature"], Value::Null);
    }

    // A constrained profile whose stored inputs touch the axis is outside
    // the class before anything is solved, and so is a Circle profile.
    let touching = root.join("touching.fcad");
    let (_, touch_labels) =
        write_revolve_document(&touching, &[[0., 0.], [10., 0.], [10., 15.], [4., 15.]]);
    // With its whole closure, so the stored inputs are the only reason.
    super::edit::rewrite_sketch(&touching, |s| {
        s.constraints.extend(closure(&touch_labels));
        s.constraints.push(horizontal(touch_labels[0]));
    });
    let circle = root.join("circle.fcad");
    write_revolve_document(&circle, &BUSHING);
    super::edit::rewrite_sketch(&circle, |s| {
        s.curves.truncate(1);
        s.curves[0].geometry = SketchGeometry::Circle {
            center: Point2::new(7., 7.).expect("centre"),
            radius: 2.,
        };
    });
    for (path, wanted) in [(&touching, "axis"), (&circle, "Lines")] {
        let edit = inspect(path)["sketches"][0]["constraint_edit"].clone();
        assert_eq!(edit["available"], false, "{edit}");
        assert!(
            edit["refusal"].as_str().expect("refusal").contains(wanted),
            "{edit}"
        );
    }

    // The writer, without a solver or a kernel: the prepared edit is
    // written only to the document it was read from, and writes exactly the
    // allowlisted cells.
    let written = root.join("written.fcad");
    std::fs::copy(&full, &written).expect("copy");
    let sketch_id = doc_sketch(&full);
    let edits = ferritecad_document::SketchConstraintEdits {
        remove: vec![],
        add: vec![ferritecad_document::AddSketchConstraint::Line(
            ferritecad_document::AddLineConstraint::Line {
                curve: labels[0],
                kind: ferritecad_document::LineConstraintKind::Horizontal,
            },
        )],
    };
    let source_doc = Document::open_read_only(&full).expect("open");
    let prepared = ferritecad_document::prepare_sketch_constraints(&source_doc, sketch_id, &edits)
        .expect("prepared");
    source_doc.close().expect("close");
    assert!(matches!(
        prepared.profile_use(),
        ferritecad_document::SketchProfileUse::FullTurnRevolve {
            axis_segment: None,
            ..
        }
    ));
    assert_eq!(prepared.added.len(), 4 + 1);
    let mut doc = Document::open(&written).expect("open");
    doc.write_sketch_constraints(&prepared)
        .expect("honest write");
    doc.close().expect("close");
    check_constraint_cells(&full, &written, sketch_id);
    // Prepared from the source, refused by a document that has moved on.
    let before = std::fs::read(&written).expect("written");
    let mut doc = Document::open(&written).expect("open");
    let error = doc
        .write_sketch_constraints(&prepared)
        .expect_err("stale preparation");
    assert!(error.to_string().contains("changed"), "{error}");
    doc.close().expect("close");
    assert_eq!(std::fs::read(&written).expect("written"), before);

    // A constrained profile keeps the other editors' deliberate policies:
    // coordinates refused, the angle of a sector offered, the Revolve's
    // stored Lines checked by the class rule.
    let c = inspect(&written);
    let row = &c["sketches"][0];
    assert_eq!(row["editable"], false);
    assert!(
        row["refusal"]
            .as_str()
            .expect("refusal")
            .contains("unconstrained")
    );
    assert_eq!(row["profile_feature"], Value::Null);
    assert_eq!(row["constraint_edit"]["available"], true);
    assert_eq!(
        row["constraint_edit"]["constraints"]
            .as_array()
            .expect("constraints")
            .len(),
        5
    );
    assert_eq!(c["revolves"][0]["profile"]["available"], true);
    assert!(
        capability_rows(&written)
            .contains(&ferritecad_document::SKETCH_CONSTRAINTS_CAPABILITY.to_owned())
    );
    let constrained_sector = root.join("constrained-sector.fcad");
    std::fs::copy(&sector, &constrained_sector).expect("copy");
    let sector_labels = sketch_of(&sector)
        .1
        .curves
        .iter()
        .map(|c| c.id)
        .collect::<Vec<_>>();
    super::edit::rewrite_sketch(&constrained_sector, |s| {
        s.constraints.push(horizontal(sector_labels[0]))
    });
    let (_, _, discovered) = target(&constrained_sector);
    assert_eq!(discovered["angle_edit"]["available"], true, "{discovered}");
    assert_eq!(discovered["profile"]["available"], true, "{discovered}");
    // Kept for a reader that is not this build (see the verification).
    if let Some(dir) = std::env::var_os("FCAD_REVOLVE_CONSTRAINT_FIXTURES") {
        let dir = Path::new(&dir);
        std::fs::create_dir_all(dir).expect("fixtures");
        for path in [&written, &constrained_sector] {
            std::fs::copy(path, dir.join(path.file_name().expect("name"))).expect("fixture");
        }
    }

    // Request shapes, refused before any kernel, in every build.
    let catalog = inspect(&full);
    let id = curve(&catalog, 0);
    let request = root.join("request.json");
    let out = root.join("never.fcad");
    let names = entries(root);
    let full_bytes = std::fs::read(&full).expect("source");
    for (why, bytes, wanted) in [
        (
            "a request array",
            format!(r#"[1,[],[{{"curve_id":{id},"rule":"horizontal"}}]]"#),
            "must be objects",
        ),
        (
            "an addition array",
            format!(r#"{{"request_version":1,"remove":[],"add":[["horizontal",{id}]]}}"#),
            "must be objects",
        ),
        (
            "a duplicate key",
            format!(
                r#"{{"request_version":1,"remove":[],"remove":[],"add":[{{"curve_id":{id},"rule":"horizontal"}}]}}"#
            ),
            "duplicate",
        ),
        (
            "an escaped duplicate key",
            format!(
                r#"{{"request_version":1,"remove":[],"add":[{{"curve_id":{id},"curve_id":{id},"rule":"horizontal"}}]}}"#
            ),
            "duplicate",
        ),
        (
            "an escaped duplicate rule",
            format!(
                r#"{{"request_version":1,"remove":[],"add":[{{"curve_id":{id},"rule":"horizontal","rule":"vertical"}}]}}"#
            ),
            "duplicate",
        ),
        (
            "an unknown field",
            format!(
                r#"{{"request_version":1,"remove":[],"add":[{{"curve_id":{id},"rule":"horizontal","axis":"y"}}]}}"#
            ),
            "unknown",
        ),
        (
            "request version 2",
            format!(
                r#"{{"request_version":2,"remove":[],"add":[{{"curve_id":{id},"rule":"horizontal"}}]}}"#
            ),
            "request_version",
        ),
    ] {
        std::fs::write(&request, bytes).expect("request");
        let v = edit_reply(
            constrain(&full, &catalog, &request, &out)
                .output()
                .expect(why),
            2,
        );
        assert!(
            v["error"]["message"]
                .as_str()
                .expect("message")
                .contains(wanted),
            "{why}: {v}"
        );
        assert!(!out.exists(), "{why}");
        let mut now = entries(root);
        now.retain(|n| n != "request.json");
        let mut then = names.clone();
        then.retain(|n| n != "request.json");
        assert_eq!(now, then, "{why}");
        assert_eq!(std::fs::read(&full).expect("source"), full_bytes, "{why}");
    }
}

/// With Open CASCADE but no solver: the class is offered, a constraint
/// copy is refused with the typed solver error and publishes nothing, and a
/// constrained document still reads, validates and refuses to rebuild.
#[test]
fn occt_without_solver_refuses_revolve_constraints_honestly() {
    if !native() {
        return;
    }
    if cfg!(feature = "planegcs") {
        eprintln!("skipped: this build links PlaneGCS; see the native constraint gates");
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let root = d.path();
    let source = create_sector(root, &SECTOR_P, SECTOR_DEG, "sector");
    let catalog = inspect(&source);
    assert_eq!(catalog["sketches"][0]["constraint_edit"]["available"], true);
    let before = std::fs::read(&source).expect("source");
    let request = root.join("request.json");
    let out = root.join("copy.fcad");
    let names = entries(root);
    write(&request, &adds(vec![rule(&catalog, 0, "horizontal")]));
    let v = edit_reply(
        constrain(&source, &catalog, &request, &out)
            .output()
            .expect("no solver"),
        2,
    );
    assert_eq!(v["error"]["kind"], "unsupported", "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .expect("message")
            .contains("planegcs"),
        "{v}"
    );
    assert!(!out.exists());
    let mut now = entries(root);
    now.retain(|n| n != "request.json");
    let mut then = names;
    then.retain(|n| n != "request.json");
    assert_eq!(now, then);
    assert_eq!(std::fs::read(&source).expect("source"), before);
    // Written directly, the constrained sector is still a document: it reads
    // and validates, and every rebuild names the missing solver.
    let labels: Vec<_> = sketch_of(&source).1.curves.iter().map(|c| c.id).collect();
    let constrained = root.join("constrained.fcad");
    std::fs::copy(&source, &constrained).expect("copy");
    super::edit::rewrite_sketch(&constrained, |s| s.constraints.push(horizontal(labels[0])));
    for args in [vec!["validate"], vec!["inspect"]] {
        let o = cli()
            .args(&args)
            .arg(&constrained)
            .arg("--json")
            .output()
            .expect("read");
        assert!(o.status.success(), "{args:?}: {o:?}");
    }
    let stl = root.join("constrained.stl");
    let o = cli()
        .arg("export-stl")
        .arg(&constrained)
        .arg("-o")
        .arg(&stl)
        .arg("--json")
        .output()
        .expect("STL");
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("planegcs"),
        "{o:?}"
    );
    assert!(!stl.exists());
    // The angle edit is offered, and its baseline rebuild names the solver.
    let (feature, version, _) = target(&constrained);
    let angle_request = root.join("angle.json");
    write_angle(&angle_request, 90.);
    let turned = root.join("turned.fcad");
    let v = angle_reply(
        angle_edit(&constrained, &feature, &version, &angle_request, &turned)
            .output()
            .expect("angle"),
        2,
    );
    assert!(
        v["error"]["message"]
            .as_str()
            .expect("message")
            .contains("planegcs"),
        "{v}"
    );
    assert!(!turned.exists());
}

/// The Coincident link at every adjacent joint, End of one Line to Start of
/// the next.
fn closure(labels: &[StableEntityId]) -> Vec<SketchConstraint> {
    (0..labels.len())
        .map(|i| SketchConstraint {
            id: StableEntityId::new(),
            rule: SketchConstraintRule::Coincident {
                a: SketchPointRef::new(labels[i], SketchPointSelector::End),
                b: SketchPointRef::new(labels[(i + 1) % labels.len()], SketchPointSelector::Start),
            },
        })
        .collect()
}

fn horizontal(curve: StableEntityId) -> SketchConstraint {
    SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Horizontal {
            a: SketchPointRef::new(curve, SketchPointSelector::Start),
            b: SketchPointRef::new(curve, SketchPointSelector::End),
        },
    }
}

fn rewrite_revolve(path: &Path, change: impl FnOnce(&mut Revolve)) {
    let mut d = Document::open(path).expect("open");
    let mut object = d
        .objects()
        .expect("objects")
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Revolve(_)))
        .expect("revolve");
    let ObjectPayload::Revolve(revolve) = &mut object.payload else {
        unreachable!()
    };
    change(revolve);
    d.write(|w| {
        w.put_object(
            object.id,
            object.parent,
            object.ordinal,
            object.name.as_deref(),
            &object.payload,
        )
    })
    .expect("rewrite");
    d.close().expect("close");
}

use super::angle::stub_sector;
use super::partial::doc_sketch;
