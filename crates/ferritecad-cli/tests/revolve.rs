// SPDX-License-Identifier: MIT
//! §27A: a full-turn Revolve made by the shipped command, measured after the
//! fact — B-Rep against Pappus, every saved name against the face its Line
//! must turn into, the cache, reopen, an independent reading of the STL, and
//! every refusal leaving storage untouched.
#![allow(clippy::panic)]
use ferritecad_document::{
    Body, CacheStore, DatumPlane, Dependency, DependencyRole, Document, EntityKind,
    FEATURE_REVOLVE_CAPABILITY, FullTurnRevolution, ObjectPayload, Point2, Revolve, RevolveAxis,
    RevolveExtent, SelectionRule, SemanticRole, Sketch, SketchCurve, SketchGeometry,
    SolidOperation, TopologyRef,
};
use ferritecad_eval::CacheOutcome;
use ferritecad_kernel::{FaceSurface, GeometryKernel, OperationContext, TessellationParams};
use ferritecad_types::{ErrorKind, ObjectId, StableEntityId, Transform};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    f64::consts::PI,
    path::{Path, PathBuf},
    process::{Command, Output},
};
#[path = "support/pipe.rs"]
mod pipe;

const OP: &str = "create-sketch-revolve";
const LINEAR_MM: f64 = 0.05;
const ANGULAR_RAD: f64 = 0.1;
/// Single-precision STL coordinates, on top of the chord budget.
const ROUNDING_MM: f64 = 1e-4;

const BUSHING: [[f64; 2]; 4] = [[4., 0.], [10., 0.], [10., 15.], [4., 15.]];
const STEPPED: [[f64; 2]; 6] = [
    [4., 0.],
    [10., 0.],
    [10., 5.],
    [7., 5.],
    [7., 15.],
    [4., 15.],
];
const SLOPED: [[f64; 2]; 4] = [[4., 0.], [10., 0.], [7., 15.], [4., 15.]];

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).expect("JSON")).expect("request");
}
fn request(path: &Path, points: &[[f64; 2]]) {
    write(
        path,
        &json!({"request_version":1,"points_mm":points,"axis":"sketch_y","angle":"full_turn"}),
    );
}
fn create(input: &Path, out: &Path) -> Command {
    let mut c = cli();
    c.arg(OP).arg(input).arg("-o").arg(out).arg("--json");
    c
}
fn reply(out: Output, code: i32) -> Value {
    assert_eq!(out.status.code(), Some(code), "{out:?}");
    assert_eq!(out.stdout.iter().filter(|&&b| b == b'\n').count(), 1);
    let v: Value = serde_json::from_slice(&out.stdout).expect("JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["operation"], OP);
    assert_eq!(v["ok"], code == 0);
    v
}
fn entries(p: &Path) -> Vec<std::ffi::OsString> {
    let mut v: Vec<_> = std::fs::read_dir(p)
        .expect("directory")
        .map(|e| e.expect("entry").file_name())
        .collect();
    v.sort();
    v
}
fn inspect(path: &Path) -> Value {
    let out = cli()
        .arg("inspect")
        .arg(path)
        .arg("--json")
        .output()
        .expect("inspect");
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice::<Value>(&out.stdout).expect("JSON")["result"].clone()
}
fn native() -> bool {
    if ferritecad_occt::is_available() {
        return true;
    }
    assert_ne!(std::env::var("FERRITECAD_REQUIRE_OCCT").as_deref(), Ok("1"));
    eprintln!("skipped: this build has no Open CASCADE");
    false
}

#[test]
fn revolve_request_refusals_and_usage_preserve_files() {
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("profile.json");
    let out = d.path().join("out.fcad");
    let missing = d.path().join("missing.json");
    let v = reply(create(&missing, &out).output().expect("process"), 2);
    assert_eq!(v["error"]["kind"], "io");
    let square = json!([[4, 0], [10, 0], [10, 15], [4, 15]]);
    for (value, kind, why) in [
        (
            json!({"request_version":2,"points_mm":square,"axis":"sketch_y","angle":"full_turn"}),
            "unsupported",
            "a later request version",
        ),
        (
            json!({"request_version":1,"points_mm":square,"axis":"sketch_y","angle":"full_turn","height_mm":10}),
            "input",
            "an extrusion field",
        ),
        (
            json!({"request_version":1,"points_mm":square,"angle":"full_turn"}),
            "input",
            "no axis",
        ),
        (
            json!({"request_version":1,"points_mm":square,"axis":"sketch_y"}),
            "input",
            "no angle",
        ),
        (
            json!({"request_version":1,"points_mm":square,"axis":"sketch_x","angle":"full_turn"}),
            "input",
            "another axis",
        ),
        (
            json!({"request_version":1,"points_mm":square,"axis":"sketch_y","angle":"half_turn"}),
            "input",
            "a partial angle",
        ),
        (
            json!({"request_version":1,"points_mm":square,"axis":"sketch_y","angle":360}),
            "input",
            "an angle as a number",
        ),
        (
            json!({"request_version":1,"points_mm":[[0,0],[10,0],[10,15],[0,15]],"axis":"sketch_y","angle":"full_turn"}),
            "input",
            "a solid shaft on the axis",
        ),
        (
            json!({"request_version":1,"points_mm":[[-1,0],[10,0],[10,15],[-1,15]],"axis":"sketch_y","angle":"full_turn"}),
            "input",
            "crossing the axis",
        ),
        (
            json!({"request_version":1,"points_mm":[[4,0],[10,15],[10,0],[4,15]],"axis":"sketch_y","angle":"full_turn"}),
            "input",
            "a bow tie",
        ),
        (
            json!({"request_version":1,"points_mm":[[4,0],[10,0]],"axis":"sketch_y","angle":"full_turn"}),
            "input",
            "two points",
        ),
        (
            json!({"request_version":1,"points_mm":[[4,0],[10,0],[10,15],[4,15],[4,0]],"axis":"sketch_y","angle":"full_turn"}),
            "input",
            "a repeated closing point",
        ),
        (
            json!({"request_version":1,"points_mm":[[4,0],[2e6,0],[10,15],[4,15]],"axis":"sketch_y","angle":"full_turn"}),
            "input",
            "beyond the coordinate limit",
        ),
        (
            json!({"request_version":1,"points_mm":[[4,0],[1e-7,0],[10,15],[4,15]],"axis":"sketch_y","angle":"full_turn"}),
            "input",
            "within the axis clearance",
        ),
    ] {
        write(&input, &value);
        let before = std::fs::read(&input).expect("input");
        let names = entries(d.path());
        let v = reply(create(&input, &out).output().expect("process"), 2);
        assert_eq!(v["error"]["kind"], kind, "{why}: {v}");
        assert_eq!(std::fs::read(&input).expect("input"), before, "{why}");
        assert_eq!(entries(d.path()), names, "{why}");
    }
    // Oversized, and not JSON at all.
    std::fs::write(&input, vec![b' '; 65537]).expect("big");
    reply(create(&input, &out).output().expect("process"), 2);
    std::fs::write(&input, b"not json").expect("text");
    reply(create(&input, &out).output().expect("process"), 2);
    request(&input, &BUSHING);
    std::fs::write(&out, b"occupied").expect("occupied");
    let v = reply(create(&input, &out).output().expect("process"), 2);
    assert_eq!(v["error"]["kind"], "input");
    assert_eq!(std::fs::read(&out).expect("output"), b"occupied");
    assert!(
        cli()
            .args([OP, "--help"])
            .output()
            .expect("help")
            .status
            .success()
    );
    let usage = cli().args([OP, "--json"]).output().expect("usage");
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
    std::fs::write(d.path().join("--json"), "bad request").expect("flag file");
    let text = cli()
        .current_dir(d.path())
        .args([OP, "-o", "new.fcad", "--", "--json"])
        .output()
        .expect("text");
    assert_eq!(text.status.code(), Some(2));
    assert!(text.stdout.is_empty());
    reply(
        create(&missing, &out)
            .stderr(pipe::closed_pipe())
            .output()
            .expect("pipe"),
        2,
    );
    assert_eq!(
        create(&missing, &out)
            .stdout(pipe::closed_pipe())
            .stderr(pipe::closed_pipe())
            .status()
            .expect("pipes")
            .code(),
        Some(7)
    );
}

#[test]
fn revolve_alias_and_stub_kernel_refusals_preserve_storage() {
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("request.json");
    request(&input, &STEPPED);
    let saved = std::fs::read(&input).expect("request");
    let alias = d.path().join("alias.fcad");
    std::fs::hard_link(&input, &alias).expect("hard link");
    for dest in [&input, &alias] {
        let names = entries(d.path());
        reply(create(&input, dest).output().expect("process"), 2);
        assert_eq!(entries(d.path()), names);
        assert_eq!(std::fs::read(&input).expect("input"), saved);
    }
    #[cfg(unix)]
    {
        let link = d.path().join("symlink.fcad");
        std::os::unix::fs::symlink(&input, &link).expect("symlink");
        reply(create(&input, &link).output().expect("process"), 2);
        assert_eq!(std::fs::read(&input).expect("input"), saved);
    }
    if !ferritecad_occt::is_available() {
        // Without a kernel a Revolve is never published: the profile is only
        // proven buildable by building it.
        let output = d.path().join("new.fcad");
        let names = entries(d.path());
        let v = reply(create(&input, &output).output().expect("stub process"), 2);
        assert_eq!(v["error"]["kind"], "unsupported");
        assert_eq!(entries(d.path()), names);
        assert_eq!(
            create(&input, &output)
                .stdout(pipe::closed_pipe())
                .stderr(pipe::closed_pipe())
                .status()
                .expect("pipes")
                .code(),
            Some(7)
        );
        assert_eq!(entries(d.path()), names);
    }
}

/// The four objects a Revolve document holds, written without a kernel.
fn write_revolve_document(path: &Path, points: &[[f64; 2]]) -> (ObjectId, Vec<StableEntityId>) {
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
                profile: sketch,
                axis: RevolveAxis::SketchY,
                extent: RevolveExtent::FullTurn,
                operation: SolidOperation::NewBody,
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
        for label in &labels {
            w.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: revolve,
                producer_feature: revolve,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::RevolveFace {
                    profile_segment: *label,
                },
                selection: SelectionRule::AllDerivedFrom { ancestor: *label },
                fallback_signature: None,
            })?;
        }
        Ok(())
    })
    .expect("revolve document");
    d.close().expect("close");
    (revolve, labels)
}

fn capability_rows(path: &Path) -> Vec<String> {
    let c = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("SQL");
    let mut s = c
        .prepare("SELECT name FROM capabilities WHERE required=1 ORDER BY name")
        .expect("query");
    s.query_map([], |r| r.get(0))
        .expect("rows")
        .collect::<rusqlite::Result<_>>()
        .expect("names")
}

#[test]
fn revolve_document_contract_and_discovery_without_kernel() {
    let d = tempfile::tempdir().expect("dir");
    let path = d.path().join("вращение space.fcad");
    let (revolve, labels) = write_revolve_document(&path, &STEPPED);
    let doc = Document::open_read_only(&path).expect("open");
    assert!(doc.validate().expect("validate").is_ok());
    let record = doc.object(revolve).expect("row").expect("revolve");
    assert_eq!(record.payload.type_name(), "feature.revolve");
    assert_eq!(record.payload.schema_version(), 1);
    assert_eq!(
        record.payload.required_capabilities(),
        vec![
            "core.part.v1".to_owned(),
            FEATURE_REVOLVE_CAPABILITY.to_owned()
        ]
    );
    doc.close().expect("close");
    assert!(capability_rows(&path).contains(&FEATURE_REVOLVE_CAPABILITY.to_owned()));

    // Discovery reports it additively and nowhere else.
    let catalog = inspect(&path);
    let revolves = catalog["revolves"].as_array().expect("revolves");
    assert_eq!(revolves.len(), 1);
    let r = &revolves[0];
    assert_eq!(r["feature_id"], revolve.to_string());
    assert_eq!(r["axis"], "sketch_y");
    assert_eq!(r["extent"], "full_turn");
    assert_eq!(r["operation"], "new_body");
    assert_eq!(r["profile"]["available"], json!(true));
    let segments = r["profile"]["segments"].as_array().expect("segments");
    assert_eq!(
        segments
            .iter()
            .map(|s| s["curve_id"].as_str().expect("id").to_owned())
            .collect::<Vec<_>>(),
        labels.iter().map(ToString::to_string).collect::<Vec<_>>()
    );
    for (s, p) in segments.iter().zip(STEPPED) {
        assert_eq!(s["start_mm"], json!(p));
    }
    assert!(
        catalog["features"].as_array().expect("features").is_empty(),
        "a Revolve is never listed where an Extrude would be"
    );
    for body in catalog["bodies"].as_array().expect("bodies") {
        for block in ["cut_edit", "cut_edit_v2", "cut_edit_v3"] {
            assert_eq!(body[block]["available"], json!(false), "{block}");
            assert!(body[block]["target"].is_null(), "{block}");
        }
    }
    for sketch in catalog["sketches"].as_array().expect("sketches") {
        assert_eq!(sketch["editable"], json!(false));
        assert!(sketch["cut_history_v3"].is_null());
    }
    let valid = cli()
        .args(["validate"])
        .arg(&path)
        .arg("--json")
        .output()
        .expect("validate");
    assert!(valid.status.success(), "{valid:?}");

    // A later layout this build does not know is preserved verbatim, the
    // document goes read-only, and nothing rebuilds or rewrites it.
    let future = d.path().join("future.fcad");
    write_revolve_document(&future, &BUSHING);
    {
        let raw = rusqlite::Connection::open(&future).expect("SQL");
        let bytes: Vec<u8> = raw
            .query_row(
                "SELECT payload FROM objects WHERE kind='feature.revolve'",
                [],
                |r| r.get(0),
            )
            .expect("payload");
        let mut envelope = ferritecad_document::Envelope::from_bytes(&bytes).expect("envelope");
        envelope
            .required_capabilities
            .push("feature.revolve.partial.v9".to_owned());
        let bytes = envelope.to_bytes().expect("bytes");
        raw.execute(
            "UPDATE objects SET payload=?1,payload_hash=?2 WHERE kind='feature.revolve'",
            rusqlite::params![
                bytes,
                ferritecad_types::ContentHash::of_bytes(&bytes)
                    .as_bytes()
                    .as_slice()
            ],
        )
        .expect("future payload");
        raw.execute(
            "INSERT INTO capabilities(name,required) VALUES('feature.revolve.partial.v9',1)",
            [],
        )
        .expect("future capability");
    }
    let before = std::fs::read(&future).expect("bytes");
    let doc = Document::open_read_only(&future).expect("still readable");
    assert!(matches!(
        doc.copy_access().expect("access"),
        ferritecad_document::Access::ReadOnly { .. }
    ));
    assert!(doc.objects().expect("objects").iter().any(|o| matches!(
        &o.payload,
        ObjectPayload::Unknown(u) if u.type_name == "feature.revolve"
    )));
    doc.close().expect("close");
    let rebuild = cli()
        .args(["rebuild", "--cold"])
        .arg(&future)
        .output()
        .expect("rebuild");
    assert!(!rebuild.status.success(), "{rebuild:?}");
    assert_eq!(std::fs::read(&future).expect("bytes"), before);

    // A RevolveFace name whose envelope omits the Revolve capability would let
    // an older build rewrite it; it is refused at the read.
    let stripped = d.path().join("stripped.fcad");
    write_revolve_document(&stripped, &BUSHING);
    {
        let raw = rusqlite::Connection::open(&stripped).expect("SQL");
        let (id, bytes): (Vec<u8>, Vec<u8>) = raw
            .query_row("SELECT id,payload FROM topology_refs LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("ref");
        let mut envelope = ferritecad_document::Envelope::from_bytes(&bytes).expect("envelope");
        envelope
            .required_capabilities
            .retain(|c| c != FEATURE_REVOLVE_CAPABILITY);
        let bytes = envelope.to_bytes().expect("bytes");
        raw.execute(
            "UPDATE topology_refs SET payload=?1,payload_hash=?2 WHERE id=?3",
            rusqlite::params![
                bytes,
                ferritecad_types::ContentHash::of_bytes(&bytes)
                    .as_bytes()
                    .as_slice(),
                id
            ],
        )
        .expect("strip");
    }
    let doc = Document::open_read_only(&stripped).expect("opens");
    let error = doc.topology_refs().expect_err("capability contract");
    assert!(
        error.to_string().contains(FEATURE_REVOLVE_CAPABILITY),
        "{error}"
    );
    doc.close().expect("close");
}

/// What one Line must turn into about the sketch (world) Y axis.
fn expected_surface(a: [f64; 2], b: [f64; 2]) -> &'static str {
    if a[1] == b[1] {
        "annulus"
    } else if a[0] == b[0] {
        "cylinder"
    } else {
        "cone"
    }
}

/// The kernel's account of a published Revolve: volume, one face per Line,
/// and every stored name resolving to the face its Line turns into.
fn measure(path: &Path, points: &[[f64; 2]], cache: Option<&[CacheOutcome]>) -> f64 {
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
    assert_eq!(built.shape(revolve), Some(shape));
    let (faces, volume) = kernel.shape_stats(shape).expect("stats");
    let exact = FullTurnRevolution::new(points.to_vec())
        .expect("policy")
        .volume_mm3();
    assert!(
        (volume - exact).abs() < 1e-6 * exact,
        "{volume} != {exact} for {points:?}"
    );
    assert_eq!(faces as usize, points.len());
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
    let mesh = kernel
        .tessellate(shape, &TessellationParams::default(), &context)
        .expect("mesh");
    let refs = d.topology_refs().expect("refs");
    assert_eq!(refs.len(), points.len(), "one name per Line");
    let mut named = std::collections::BTreeSet::new();
    for reference in &refs {
        let SemanticRole::RevolveFace { profile_segment } = reference.output_role else {
            panic!("only RevolveFace names")
        };
        let resolved = built.resolve(reference).expect("resolves");
        let [face] = resolved.as_slice() else {
            panic!("one face for {profile_segment}: {resolved:?}")
        };
        assert!(named.insert(*face), "two names on one face");
        let (a, b) = lines[&profile_segment];
        let range = mesh.faces.iter().find(|r| r.face == *face).expect("drawn");
        let vertices: Vec<[f64; 3]> = mesh.indices
            [range.first_index as usize..(range.first_index + range.index_count) as usize]
            .iter()
            .map(|v| std::array::from_fn(|j| f64::from(mesh.positions[*v as usize * 3 + j])))
            .collect();
        let radius = |p: &[f64; 3]| p[0].hypot(p[2]);
        let surface = kernel.face_surface(*face).expect("surface");
        match expected_surface(a, b) {
            "annulus" => {
                assert_eq!(surface, FaceSurface::Plane);
                assert!(vertices.iter().all(|p| (p[1] - a[1]).abs() < 1e-6));
                let (lo, hi) = (a[0].min(b[0]), a[0].max(b[0]));
                assert!(
                    vertices
                        .iter()
                        .all(|p| radius(p) > lo - LINEAR_MM && radius(p) < hi + 1e-6)
                );
            }
            "cylinder" => {
                assert_eq!(surface, FaceSurface::Cylinder { radius: a[0] });
                let (origin, direction) = kernel.surface_axis(*face).expect("axis");
                assert!(origin[0].abs() < 1e-9 && origin[2].abs() < 1e-9);
                assert!((direction[1].abs() - 1.).abs() < 1e-12);
                assert!(vertices.iter().all(|p| (radius(p) - a[0]).abs() < 1e-4));
            }
            _ => {
                assert_eq!(surface, FaceSurface::Cone);
                let (origin, direction) = kernel.surface_axis(*face).expect("axis");
                assert!(origin[0].abs() < 1e-9 && origin[2].abs() < 1e-9);
                assert!((direction[1].abs() - 1.).abs() < 1e-12);
                for p in &vertices {
                    let t = (p[1] - a[1]) / (b[1] - a[1]);
                    assert!((radius(p) - (a[0] + t * (b[0] - a[0]))).abs() < LINEAR_MM);
                }
            }
        }
        // Every name is a face that goes all the way round.
        for (sx, sz) in [(1., 1.), (-1., 1.), (-1., -1.), (1., -1.)] {
            assert!(
                vertices
                    .iter()
                    .any(|p| p[0] * sx > 1e-3 && p[2] * sz > 1e-3)
            );
        }
    }
    assert_eq!(named.len(), faces as usize, "every face has one name");
    // An extrusion's name for the same Line never resolves on a Revolve.
    let (label, _) = lines.iter().next().expect("a Line");
    let wrong = TopologyRef {
        id: StableEntityId::new(),
        owner: revolve,
        producer_feature: revolve,
        expected_kind: ferritecad_document::EntityKind::Face,
        output_role: SemanticRole::ExtrudeSide {
            profile_segment: *label,
        },
        selection: SelectionRule::AllDerivedFrom { ancestor: *label },
        fallback_signature: None,
    };
    assert_eq!(
        built.resolve(&wrong).expect_err("not a swept face").kind(),
        ErrorKind::Topology
    );
    built.release_all(&mut kernel);
    d.close().expect("close");
    assert_eq!(kernel.live_shape_count(), 0);
    volume
}

/// An independent reading of the exported binary STL.
struct Mesh {
    volume: f64,
    faces: Vec<[[f64; 3]; 3]>,
}
fn mesh(path: &Path, out: &Path) -> Mesh {
    let r = cli()
        .arg("export-stl")
        .arg(path)
        .arg("-o")
        .arg(out)
        .args(["--linear-deflection", &LINEAR_MM.to_string()])
        .args(["--angular-deflection", &ANGULAR_RAD.to_string()])
        .arg("--json")
        .output()
        .expect("STL");
    assert!(r.status.success(), "{r:?}");
    let b = std::fs::read(out).expect("STL bytes");
    let count = u32::from_le_bytes(b[80..84].try_into().expect("count")) as usize;
    assert_eq!(b.len(), 84 + 50 * count);
    let mut six = 0.;
    let mut faces = Vec::new();
    for t in b[84..].chunks_exact(50) {
        let mut p = [[0.; 3]; 3];
        for (i, point) in p.iter_mut().enumerate() {
            for (j, value) in point.iter_mut().enumerate() {
                let k = 12 + 12 * i + 4 * j;
                *value = f64::from(f32::from_le_bytes(t[k..k + 4].try_into().expect("f32")));
                assert!(value.is_finite());
            }
        }
        let [a, b, c] = p;
        six += a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
        faces.push(p);
    }
    Mesh {
        volume: six / 6.,
        faces,
    }
}

/// Closed and oriented; a clear central channel; every vertex inside the
/// turned profile; the volume inside what chord deviation can do to it.
fn check_mesh(m: &Mesh, points: &[[f64; 2]]) {
    let q = |v: [f64; 3]| v.map(|x| (x * 1e4).round() as i64);
    let mut directed: BTreeMap<_, usize> = BTreeMap::new();
    for face in &m.faces {
        let p: Vec<_> = face.iter().map(|v| q(*v)).collect();
        for k in 0..3 {
            *directed.entry((p[k], p[(k + 1) % 3])).or_default() += 1;
        }
    }
    assert!(
        directed.values().all(|n| *n == 1),
        "not one oriented surface"
    );
    for (a, b) in directed.keys() {
        assert!(directed.contains_key(&(*b, *a)), "open mesh");
    }
    let r_min = points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    // Every vertex, turned back into the profile plane, is inside the profile
    // or on its boundary, within the chord budget: no material near the axis,
    // none past the step or the sloped wall.
    let inside = |r: f64, y: f64| {
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
        odd || near < LINEAR_MM + ROUNDING_MM
    };
    for face in &m.faces {
        for v in face {
            let r = v[0].hypot(v[2]);
            assert!(
                r > r_min - LINEAR_MM - ROUNDING_MM,
                "material near the axis: {v:?}"
            );
            assert!(inside(r, v[1]), "{v:?} is outside the turned profile");
        }
    }
    // Chord deviation moves every face of revolution by at most LINEAR_MM
    // toward the axis. Bounding each Line's swept band by that gives a band
    // around the analytic volume, independent of the kernel's own numbers.
    let exact = FullTurnRevolution::new(points.to_vec())
        .expect("policy")
        .volume_mm3();
    let band: f64 = (0..points.len())
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            2. * PI * a[0].max(b[0]) * LINEAR_MM * (b[1] - a[1]).abs()
        })
        .sum();
    assert!(
        (m.volume - exact).abs() <= band,
        "mesh volume {} not within {band} of {exact}",
        m.volume
    );
}

fn variants(points: &[[f64; 2]]) -> Vec<Vec<[f64; 2]>> {
    let shifted: Vec<_> = points.iter().map(|[x, y]| [*x, y + 2.625]).collect();
    let mut reversed = points.to_vec();
    reversed.reverse();
    let mut both = shifted.clone();
    both.reverse();
    vec![points.to_vec(), reversed, shifted, both]
}

#[test]
fn native_revolve_geometry_names_cache_reopen_and_exports() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let mut artifact = 0;
    for (name, points, analytic) in [
        ("bushing", &BUSHING[..], PI * (100. - 16.) * 15.),
        ("stepped", &STEPPED[..], 750. * PI),
        ("sloped", &SLOPED[..], 855. * PI),
    ] {
        for (i, variant) in variants(points).into_iter().enumerate() {
            let input = d.path().join(format!("{name}-{i}.json"));
            let out = d.path().join(format!("{name}-{i}.fcad"));
            request(&input, &variant);
            let v = reply(create(&input, &out).output().expect("create"), 0);
            assert_eq!(v["result"]["destination"], out.to_str().expect("utf-8"));
            let volume = measure(&out, &variant, None);
            assert!((volume - analytic).abs() < 1e-6 * analytic, "{name}-{i}");
            check_mesh(&mesh(&out, &out.with_extension("stl")), &variant);
            if i == 0 && artifact < 2 && name != "bushing" {
                if let Some(dir) = std::env::var_os("FCAD_REVOLVE_ARTIFACTS") {
                    let dir = Path::new(&dir);
                    std::fs::create_dir_all(dir).expect("artifacts");
                    let fbx = dir.join(format!("revolve-{artifact}.fbx"));
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

    // Reopen, cold, and the cache: fill, hit, then change the profile under
    // the same sidecar and miss with new geometry.
    let input = d.path().join("cache.json");
    let path = d.path().join("cache.fcad");
    request(&input, &STEPPED);
    reply(create(&input, &path).output().expect("create"), 0);
    let cold = measure(&path, &STEPPED, None);
    use CacheOutcome::{Hit, Miss};
    // A person reading the stored references sees each one named after its
    // own Line, all resolved.
    let listed = cli()
        .arg("print-topology")
        .arg(&path)
        .output()
        .expect("print-topology");
    assert!(listed.status.success(), "{listed:?}");
    let listed = String::from_utf8(listed.stdout).expect("utf-8");
    let lines = inspect(&path)["result"]["revolves"][0]["profile"]["segments"]
        .as_array()
        .expect("segments")
        .iter()
        .map(|s| s["curve_id"].as_str().expect("id").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), STEPPED.len());
    for id in &lines {
        assert!(
            listed.contains(&format!("revolve face from segment {id}")),
            "{listed}"
        );
    }
    assert!(!listed.contains("unknown semantic role"), "{listed}");
    assert!(listed.contains("6 of 6 references resolved"), "{listed}");
    assert_eq!(cold, measure(&path, &STEPPED, Some(&[Miss])));
    assert_eq!(cold, measure(&path, &STEPPED, Some(&[Hit])));
    let wider: Vec<[f64; 2]> = STEPPED
        .iter()
        .map(|[x, y]| [if *x == 10. { 11. } else { *x }, *y])
        .collect();
    {
        let mut doc = Document::open(&path).expect("writable");
        let sketch = doc
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("sketch");
        let ObjectPayload::Sketch(mut drawing) = sketch.payload.clone() else {
            unreachable!()
        };
        let n = wider.len();
        for (i, curve) in drawing.curves.iter_mut().enumerate() {
            curve.geometry = SketchGeometry::Line {
                start: Point2::new(wider[i][0], wider[i][1]).expect("point"),
                end: Point2::new(wider[(i + 1) % n][0], wider[(i + 1) % n][1]).expect("point"),
            };
        }
        doc.write(|w| {
            w.put_object(
                sketch.id,
                sketch.parent,
                sketch.ordinal,
                sketch.name.as_deref(),
                &ObjectPayload::Sketch(drawing),
            )
        })
        .expect("the profile changes");
        doc.close().expect("close");
    }
    let grown = measure(&path, &wider, Some(&[Miss]));
    assert!(grown > cold, "{grown} > {cold}");
    assert_eq!(grown, measure(&path, &wider, Some(&[Hit])));
    assert_eq!(grown, measure(&path, &wider, None));
}

/// A kernel and no solver: an unconstrained profile needs no planegcs.
#[test]
fn occt_without_solver_revolves_a_profile() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("request.json");
    let out = d.path().join("bushing.fcad");
    request(&input, &SLOPED);
    reply(create(&input, &out).output().expect("create"), 0);
    measure(&out, &SLOPED, None);
}

#[test]
fn native_revolve_publication_delivery_and_cancellation() {
    if !native() {
        return;
    }
    let d = tempfile::tempdir().expect("dir");
    let input = d.path().join("pipe.json");
    request(&input, &STEPPED);
    for both in [false, true] {
        let published = d.path().join(format!("pipe-{both}.fcad"));
        let mut c = create(&input, &published);
        c.stdout(pipe::closed_pipe());
        if both {
            c.stderr(pipe::closed_pipe());
        } else {
            c.stderr(std::process::Stdio::piped());
        }
        let delivery = c.output().expect("closed stdout");
        assert_eq!(delivery.status.code(), Some(7));
        if !both {
            assert!(
                String::from_utf8_lossy(&delivery.stderr).contains("JSON report delivery failed")
            );
        }
        let doc = Document::open_read_only(&published).expect("publication survived");
        assert_eq!(doc.objects().expect("objects").len(), 4);
        doc.close().expect("close");
    }
    // Cancelled on the shared job: nothing published, no scratch left.
    let names = entries(d.path());
    let token = ferritecad_kernel::CancelToken::new();
    token.cancel();
    let never: PathBuf = d.path().join("never.fcad");
    let error = ferritecad_jobs::create_document_with_kernel(
        ferritecad_jobs::CreateDocumentRequest::new(
            &never,
            ferritecad_jobs::NewDocument::SketchRevolve(
                FullTurnRevolution::new(STEPPED.to_vec()).expect("policy"),
            ),
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
