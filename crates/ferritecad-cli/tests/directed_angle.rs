// SPDX-License-Identifier: MIT
//! §31F: a directed angle between two saved Lines, through the shipped
//! command `edit`.
//!
//! Every angle asserted on a solved profile is measured here, from the solved
//! points of the published file after it was reopened and rebuilt cold, with
//! `atan2(cross, dot)` written out again below — never read back from the
//! request, the stored radians or the solver. The profiles are chosen so that
//! `+θ` and `-θ` give different shapes: a dart whose reflex corner turns the
//! other way round, and a quadrilateral whose top edge tilts either way off
//! antiparallel.
#![allow(clippy::panic)]
#[path = "support/names.rs"]
mod names;
#[path = "support/transaction.rs"]
mod transaction;
use ferritecad_document::{CacheStore, Document, ObjectPayload, SketchGeometry};
use ferritecad_eval::{CacheOutcome, rebuild_cached, rebuild_cold};
use ferritecad_kernel::{GeometryKernel, OperationContext};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};
#[path = "support/pipe.rs"]
mod pipe;

const OP: &str = "edit";
/// Solved and measured angles agree to well inside this, in degrees.
const DEGREES: f64 = 1e-6;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferritecad"))
}
fn reply(o: Output, op: &str, code: i32) -> Value {
    assert_eq!(o.status.code(), Some(code), "{o:?}");
    assert_eq!(o.stdout.iter().filter(|&&b| b == b'\n').count(), 1, "{o:?}");
    let v: Value = serde_json::from_slice(&o.stdout).expect("one JSON object");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["operation"], op);
    assert_eq!(v["ok"], code == 0);
    assert_eq!(v.get("result").is_some(), code == 0);
    assert_eq!(v.get("error").is_some(), code != 0);
    v
}
fn inspect(p: &Path) -> Value {
    reply(
        cli()
            .arg("inspect")
            .arg(p)
            .arg("--json")
            .output()
            .expect("inspect"),
        "inspect",
        0,
    )["result"]
        .clone()
}
fn write(p: &Path, v: &Value) {
    std::fs::write(p, serde_json::to_vec(v).expect("encode")).expect("write");
}
fn entries(p: &Path) -> Vec<std::ffi::OsString> {
    let mut v: Vec<_> = std::fs::read_dir(p)
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    v.sort();
    v
}
/// Whether this build has what the gate needs, each missing piece excused only
/// where that piece is not required.
fn have(kernel: bool, solver: bool) -> bool {
    ferritecad_fixtures::native_dependencies(&[
        (!kernel || ferritecad_occt::is_available(), "OCCT"),
        (!solver || ferritecad_eval::solver_available(), "PLANEGCS"),
    ])
}

/// A saved XY Line profile written by the shipped creation command.
struct Source {
    root: tempfile::TempDir,
    path: PathBuf,
    request: PathBuf,
    catalog: Value,
}
impl Source {
    fn new(points: &[[f64; 2]]) -> Self {
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("контур space.fcad");
        let request = root.path().join("request.json");
        write(
            &request,
            &json!({"request_version":1,"points_mm":points,"height_mm":10}),
        );
        reply(
            cli()
                .arg("create-sketch-extrude")
                .arg(&request)
                .arg("-o")
                .arg(&path)
                .arg("--json")
                .output()
                .expect("create"),
            "create-sketch-extrude",
            0,
        );
        let catalog = inspect(&path);
        Self {
            root,
            path,
            request,
            catalog,
        }
    }
    /// The sample plate, made with no kernel and no solver: what a build with
    /// neither can still read, edit and refuse on.
    fn sample() -> Self {
        let root = tempfile::tempdir().expect("dir");
        let path = root.path().join("плита space.fcad");
        reply(
            cli()
                .arg("create")
                .arg(&path)
                .args(["--sample", "--json"])
                .output()
                .expect("create"),
            "create",
            0,
        );
        let catalog = inspect(&path);
        Self {
            request: root.path().join("request.json"),
            root,
            path,
            catalog,
        }
    }
    /// The slanted quadrilateral every older Line gate uses.
    fn slanted() -> Self {
        Self::new(&[[-20., -10.], [40., -8.], [42., 30.], [-20., 30.]])
    }
    /// A dart: the corner between Lines 1 and 2 is reflex, turning -53°.
    fn dart() -> Self {
        Self::new(&[[0., 0.], [40., 0.], [10., 10.], [0., 40.]])
    }
    fn rectangle() -> Self {
        Self::new(&[[0., 0.], [60., 0.], [60., 30.], [0., 30.]])
    }
    fn curve(&self, i: usize) -> Value {
        self.catalog["sketches"][0]["curves"][i]["curve_id"].clone()
    }
    /// One vector of a request: Line `i` read `from` -> `to`.
    fn vector(&self, i: usize, from: &str, to: &str) -> Value {
        json!({"from":transaction::point_ref(&self.curve(i),from),"to":transaction::point_ref(&self.curve(i),to)})
    }
    /// A request that adds `add` and removes `remove`, from `catalog`'s snapshot of `from`.
    fn command(
        &self,
        from: &Path,
        catalog: &Value,
        remove: &[Value],
        add: &[Value],
        out: &Path,
    ) -> Command {
        write(
            &self.request,
            &transaction::constraints(&catalog["sketches"][0], remove, add),
        );
        transaction::command(
            from,
            catalog["content_version"].as_str().expect("version"),
            &self.request,
            out,
        )
    }
    /// Publishes `add` onto the source and answers the result.
    fn publish(&self, add: &[Value], name: &str) -> (PathBuf, Value) {
        let out = self.root.path().join(name);
        reply(
            self.command(&self.path, &self.catalog, &[], add, &out)
                .output()
                .expect("publish"),
            OP,
            0,
        );
        let facts = facts(&out);
        (out, facts)
    }
    fn refused(&self, from: &Path, catalog: &Value, add: &[Value]) -> Value {
        let out = self.root.path().join("never.fcad");
        let before = entries(self.root.path());
        let v = reply(
            self.command(from, catalog, &[], add, &out)
                .output()
                .expect("refusal"),
            OP,
            2,
        );
        assert!(!out.exists(), "a refusal published {out:?}");
        assert_eq!(entries(self.root.path()), before, "a refusal left files");
        v["error"].clone()
    }
}

fn angle(a: Value, b: Value, degrees: f64) -> Value {
    json!({"kind":"directed_angle","a":a,"b":b,"angle":transaction::number(degrees,"deg")})
}
/// The counter-clockwise turn from `u` to `v`, in degrees.
fn turn(u: [f64; 2], v: [f64; 2]) -> f64 {
    (u[0] * v[1] - u[1] * v[0])
        .atan2(u[0] * v[0] + u[1] * v[1])
        .to_degrees()
}

/// One published file, reopened and rebuilt cold.
struct Solved {
    vectors: Vec<[f64; 2]>,
    dof: usize,
    redundant: usize,
    area: f64,
}
impl Solved {
    /// Line `i` read start -> end, or end -> start.
    fn dir(&self, i: usize, forward: bool) -> [f64; 2] {
        let v = self.vectors[i];
        if forward { v } else { [-v[0], -v[1]] }
    }
    /// The turn from stored Line `i` to stored Line `j`, both start -> end.
    fn turn(&self, i: usize, j: usize) -> f64 {
        turn(self.vectors[i], self.vectors[j])
    }
}
fn solved(path: &Path) -> Solved {
    let d = Document::open_read_only(path).expect("reopen");
    let objects = d.objects().expect("objects");
    let sketch = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("sketch");
    let ObjectPayload::Sketch(stored) = &sketch.payload else {
        panic!("sketch")
    };
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let built = rebuild_cold(&d, &mut kernel, &OperationContext::default())
        .expect("a cold rebuild after reopening");
    let report = built.solve_report(sketch.id).expect("solve report");
    let (dof, redundant) = (report.degrees_of_freedom(), report.redundant().len());
    let presentation = built.sketch_presentation(sketch.id).expect("presentation");
    let (mut starts, mut ends) = (vec![], vec![]);
    for (curve, saved) in presentation.curves().iter().zip(&stored.curves) {
        assert_eq!(curve.id(), saved.id, "the solve kept every UUID and order");
        let SketchGeometry::Line { start, end } = curve.geometry() else {
            panic!("a Line")
        };
        starts.push([start.x, start.y]);
        ends.push([end.x, end.y]);
    }
    for (i, end) in ends.iter().enumerate() {
        let next = starts[(i + 1) % starts.len()];
        assert!(
            (end[0] - next[0]).abs() < 1e-6 && (end[1] - next[1]).abs() < 1e-6,
            "joint {i} is open: {end:?} {next:?}"
        );
    }
    crate::names::assert_primary(&built);
    built.release_all(&mut kernel);
    d.close().expect("close");
    let vectors: Vec<_> = starts
        .iter()
        .zip(&ends)
        .map(|(s, e)| [e[0] - s[0], e[1] - s[1]])
        .collect();
    for (i, v) in vectors.iter().enumerate() {
        assert!(v[0].hypot(v[1]) > 1., "Line {i} collapsed: {v:?}");
    }
    let area = starts
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let b = starts[(i + 1) % starts.len()];
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>()
        / 2.;
    Solved {
        vectors,
        dof,
        redundant,
        area,
    }
}

/// The solid's bytes, read independently of the kernel: the STL integrates to
/// the solved polygon's area times the height, and the FBX is complete. Keeps
/// the FBX where a CI step can hand it to the pinned reader.
fn exports(path: &Path, solved: &Solved, tag: &str) {
    let stl = path.with_extension("stl");
    let v = reply(
        cli()
            .arg("export-stl")
            .arg(path)
            .arg("-o")
            .arg(&stl)
            .arg("--json")
            .output()
            .expect("STL"),
        "export-stl",
        0,
    );
    let bytes = std::fs::read(&stl).expect("STL bytes");
    let n = u32::from_le_bytes(bytes[80..84].try_into().expect("count")) as usize;
    assert_eq!(bytes.len(), 84 + 50 * n);
    assert_eq!(v["result"]["triangles"], n);
    let mut six = 0.;
    for t in bytes[84..].chunks_exact(50) {
        let mut p = [[0.; 3]; 3];
        for (i, vertex) in p.iter_mut().enumerate() {
            for (j, c) in vertex.iter_mut().enumerate() {
                let at = 12 + 12 * i + 4 * j;
                *c = f64::from(f32::from_le_bytes(
                    t[at..at + 4].try_into().expect("coordinate"),
                ));
            }
        }
        let [a, b, c] = p;
        six += a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
    }
    let volume = six.abs() / 6.;
    assert!(
        (volume - solved.area.abs() * 10.).abs() < solved.area.abs() * 10. * 1e-5,
        "the exported solid is {volume} mm3 and the solved polygon says {}",
        solved.area.abs() * 10.
    );
    let fbx = path.with_extension("fbx");
    let r = reply(
        cli()
            .arg("export-fbx")
            .arg(path)
            .arg("-o")
            .arg(&fbx)
            .arg("--json")
            .output()
            .expect("FBX"),
        "export-fbx",
        0,
    );
    assert_eq!(r["result"]["complete"], true);
    if let Some(dir) = std::env::var_os("FCAD_DIRECTED_ANGLE_ARTIFACT_DIR") {
        std::fs::create_dir_all(&dir).expect("artifacts");
        std::fs::copy(&fbx, Path::new(&dir).join(format!("angle-{tag}.fbx"))).expect("artifact");
    }
}

fn constraint_ids(catalog: &Value) -> Vec<Value> {
    catalog["sketches"][0]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .map(|c| c["constraint_id"].clone())
        .collect()
}

/// Discovery: a stored angle is reported with its directions and its degrees,
/// the older relations are reported exactly as before, and the capability is in
/// the file. The angle is written through the document, so neither a kernel nor
/// a solver is needed to read it back.
#[test]
fn discovery_reports_a_stored_directed_angle_and_leaves_the_older_forms_alone() {
    use ferritecad_document::{
        AddLineConstraint, AddSketchConstraint, DirectedAngle, DirectedLine, LineEndpoint,
        LineRelation, SketchConstraintEdits,
    };
    let s = Source::sample();
    let ids: Vec<ferritecad_types::StableEntityId> = s.catalog["sketches"][0]["curves"]
        .as_array()
        .expect("curves")
        .iter()
        .map(|c| c["curve_id"].as_str().expect("id").parse().expect("UUID"))
        .collect();
    let with = s.root.path().join("stored.fcad");
    std::fs::copy(&s.path, &with).expect("copy");
    let mut d = Document::open(&with).expect("open");
    let sketch = d
        .objects()
        .expect("objects")
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("sketch")
        .id;
    let line = |i: usize, from: LineEndpoint| {
        DirectedLine::new(ids[i], from, from.other()).expect("a vector")
    };
    let prepared = ferritecad_fixtures::constraint_changes(
        &d,
        sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![
                AddSketchConstraint::Line(AddLineConstraint::DirectedAngle {
                    a: line(0, LineEndpoint::End),
                    b: line(1, LineEndpoint::Start),
                    angle: DirectedAngle::from_degrees(-30.).expect("angle"),
                }),
                AddSketchConstraint::Line(AddLineConstraint::Relation {
                    a: ids[2],
                    b: ids[3],
                    relation: LineRelation::Parallel,
                }),
            ],
        },
    )
    .expect("prepared");
    let (angle_id, parallel_id) = (
        ferritecad_fixtures::added_constraints(&prepared)[4].id,
        ferritecad_fixtures::added_constraints(&prepared)[5].id,
    );
    d.write_changes(&prepared).expect("written");
    d.close().expect("close");

    let found = inspect(&with);
    let edit = &found["sketches"][0];

    let rules: Vec<_> = edit["constraints"].as_array().expect("constraints").clone();
    let by_id = |id: &ferritecad_types::StableEntityId| {
        rules
            .iter()
            .find(|c| c["constraint_id"] == json!(id.to_string()))
            .expect("listed")["rule"]
            .clone()
    };
    let end = |i: usize, at: &str| json!({"curve_id": ids[i].to_string(), "at": at});
    assert_eq!(
        by_id(&angle_id),
        json!({"kind":"directed_angle",
               "a":{"from":end(0,"end"),"to":end(0,"start")},
               "b":{"from":end(1,"start"),"to":end(1,"end")},
               "angle":{"value":-30.0,"unit":"deg"}}),
        "both directions as said, the value in degrees as typed"
    );
    // The older relation keeps exactly the shape it always had.
    assert_eq!(
        by_id(&parallel_id),
        json!({"kind":"parallel",
               "a":{"from":end(2,"start"),"to":end(2,"end")},
               "b":{"from":end(3,"start"),"to":end(3,"end")}})
    );
    let valid = reply(
        cli()
            .arg("validate")
            .arg(&with)
            .arg("--json")
            .output()
            .expect("validate"),
        "validate",
        0,
    );
    assert_eq!(valid["result"]["valid"], true, "{valid}");
    // Inspect must report the persisted angle without quantizing it to 1e-9°.
    // Use the document writer so this discovery check also runs without native
    // libraries, and retain the unrelated Parallel throughout replacements.
    let mut current = angle_id;
    for degrees in [1e-10, -1e-10, 12.345678901234, -179.9999999999] {
        let requested = DirectedAngle::from_degrees(degrees).expect("precise angle");
        let mut d = Document::open(&with).expect("reopen");
        let replacement = ferritecad_fixtures::constraint_changes(
            &d,
            sketch,
            &SketchConstraintEdits {
                remove: vec![current],
                add: vec![AddSketchConstraint::Line(
                    AddLineConstraint::DirectedAngle {
                        a: line(0, LineEndpoint::End),
                        b: line(1, LineEndpoint::Start),
                        angle: requested,
                    },
                )],
            },
        )
        .expect("replace");
        current = ferritecad_fixtures::added_constraints(&replacement)[0].id;
        d.write_changes(&replacement).expect("write precise angle");
        d.close().expect("close");
        let read = inspect(&with);
        let reported = read["sketches"][0]["constraints"]
            .as_array()
            .expect("constraints")
            .iter()
            .find(|c| c["constraint_id"] == json!(current))
            .expect("angle")["rule"]["angle"]["value"]
            .as_f64()
            .expect("numeric angle");
        assert!(reported > -180. && reported <= 180., "{reported}");
        assert!(
            (reported - degrees).abs() <= degrees.abs() * 4. * f64::EPSILON,
            "inspect reported {reported} for {degrees}"
        );
    }
}

/// OCCT without a solver: a document that holds an angle is neither recomputed
/// nor published as solved, and the older route (an unconstrained document, its
/// coordinates and its exports) works exactly as before.
#[test]
fn without_a_solver_an_angled_document_is_not_solved_and_the_unconstrained_route_works() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
        || ferritecad_eval::solver_available()
    {
        return;
    }
    use ferritecad_document::{
        AddLineConstraint, AddSketchConstraint, DirectedAngle, DirectedLine, LineEndpoint,
        SketchConstraintEdits,
    };
    let s = Source::slanted();
    // The unconstrained route: a cold rebuild, validation and an export.
    for args in [["rebuild", "--cold"], ["validate", "--"]] {
        let mut c = cli();
        c.arg(args[0]).arg(&s.path);
        if args[1] == "--cold" {
            c.arg("--cold");
        }
        let o = c.output().expect(args[0]);
        assert!(o.status.success(), "{}: {o:?}", args[0]);
    }
    let plain = s.root.path().join("plain.stl");
    reply(
        cli()
            .arg("export-stl")
            .arg(&s.path)
            .arg("-o")
            .arg(&plain)
            .arg("--json")
            .output()
            .expect("STL"),
        "export-stl",
        0,
    );
    // An angle stored through the document.
    let with = s.root.path().join("angled.fcad");
    std::fs::copy(&s.path, &with).expect("copy");
    let mut d = Document::open(&with).expect("open");
    let sketch = d
        .objects()
        .expect("objects")
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("sketch")
        .id;
    let ids: Vec<ferritecad_types::StableEntityId> = (0..2)
        .map(|i| s.curve(i).as_str().expect("id").parse().expect("UUID"))
        .collect();
    let v =
        |i: usize| DirectedLine::new(ids[i], LineEndpoint::Start, LineEndpoint::End).expect("v");
    let prepared = ferritecad_fixtures::constraint_changes(
        &d,
        sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![AddSketchConstraint::Line(
                AddLineConstraint::DirectedAngle {
                    a: v(0),
                    b: v(1),
                    angle: DirectedAngle::from_degrees(60.).expect("angle"),
                },
            )],
        },
    )
    .expect("prepared");
    d.write_changes(&prepared).expect("written");
    d.close().expect("close");
    // Nothing solves it and nothing is published as if it had been.
    let stl = s.root.path().join("angled.stl");
    let o = cli()
        .arg("export-stl")
        .arg(&with)
        .arg("-o")
        .arg(&stl)
        .arg("--json")
        .output()
        .expect("STL");
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    assert!(!stl.exists());
    let catalog = inspect(&with);
    let error = s.refused(
        &with,
        &catalog,
        &[angle(
            s.vector(2, "start", "end"),
            s.vector(3, "start", "end"),
            90.,
        )],
    );
    assert_eq!(error["kind"], "unsupported", "{error}");
}

/// What a published copy must still be, whatever angle it carries: the source's
/// document, objects, links and references, with the Sketch's plane, curves,
/// name and place untouched; only the constraint list is new.
fn stored_same(a: &Path, b: &Path) {
    let a = Document::open_read_only(a).expect("source");
    let b = Document::open_read_only(b).expect("copy");
    assert_eq!(a.meta().document_id, b.meta().document_id);
    assert_eq!(
        a.dependencies().expect("deps"),
        b.dependencies().expect("deps")
    );
    assert_eq!(crate::names::snapshot(&a), crate::names::snapshot(&b));
    for o in a.objects().expect("objects") {
        let n = b.object(o.id).expect("read").expect("same UUID");
        if let (ObjectPayload::Sketch(s), ObjectPayload::Sketch(t)) = (&o.payload, &n.payload) {
            assert_eq!(s.plane, t.plane);
            assert_eq!(s.curves, t.curves, "the stored coordinates are the guess");
            assert_eq!(
                (&o.name, o.parent, o.ordinal),
                (&n.name, n.parent, n.ordinal)
            );
        } else {
            assert_eq!(o, n, "another object changed");
        }
    }
    assert!(b.validate().expect("validate").is_ok());
    a.close().expect("close");
    b.close().expect("close");
}

/// The four corners' turns of a closed profile, each from Line `i` to Line
/// `i + 1`, in stored order.
fn corners(solved: &Solved) -> Vec<f64> {
    (0..solved.vectors.len())
        .map(|i| solved.turn(i, (i + 1) % solved.vectors.len()))
        .collect()
}
fn near(x: f64, y: f64, what: &str) {
    assert!((x - y).abs() < DEGREES, "{what}: {x}° is not {y}°");
}
fn flat_modified(path: &Path) -> std::time::SystemTime {
    std::fs::metadata(path)
        .expect("meta")
        .modified()
        .expect("mtime")
}

/// A dart whose reflex corner is the one the angle is about, said five ways.
/// The profile separates `+θ` from `-θ`: `-70°` keeps the reflex corner, `+50°`
/// turns it into a convex one.
#[test]
fn native_directed_angle_signs_directions_and_equivalent_spellings_on_a_dart() {
    if !have(true, true) {
        return;
    }
    let s = Source::dart();
    let (bytes, mtime) = (
        std::fs::read(&s.path).expect("source"),
        flat_modified(&s.path),
    );
    let stored = corners(&{
        // The stored dart is its own guess: corner 1 -> 2 turns -53°.
        let one = s.publish(
            &[transaction::line_rule(&s.curve(0), "horizontal")],
            "free.fcad",
        );
        solved(&one.0)
    });
    assert!(stored[1] < 0., "the dart has a reflex corner: {stored:?}");
    let (e, st) = ("end", "start");
    let side = |i: usize, f: &str, t: &str| s.vector(i, f, t);
    let spellings = [
        ("A to B", side(1, st, e), side(2, st, e), -70.),
        ("B to A, swapped", side(2, st, e), side(1, st, e), 70.),
        ("A turned", side(1, e, st), side(2, st, e), 110.),
        ("B turned", side(1, st, e), side(2, e, st), 110.),
        ("both turned", side(1, e, st), side(2, e, st), -70.),
    ];
    let mut first = None;
    for (i, (name, a, b, degrees)) in spellings.iter().enumerate() {
        let (path, result) = s.publish(
            &[angle(a.clone(), b.clone(), *degrees)],
            &format!("dart-{i}.fcad"),
        );
        let added = result["constraints"].as_array().expect("added");
        assert_eq!(added.len(), 5, "four joints and the angle");
        // Published exactly as said: both directions and the typed value.
        let rule = &added[4]["rule"];
        let end = |v: &Value, key: &str| json!({"curve_id": v[key]["curve"], "at": v[key]["at"]});
        assert_eq!(
            *rule,
            json!({"kind":"directed_angle",
                   "a":{"from":end(a,"from"),"to":end(a,"to")},
                   "b":{"from":end(b,"from"),"to":end(b,"to")},
                   "angle":{"value":*degrees,"unit":"deg"}}),
            "{name}"
        );
        assert_eq!(result["solve"]["degrees_of_freedom"], 7, "{name}");
        assert_eq!(
            result["solve"]["redundant_constraint_ids"],
            json!([]),
            "{name}"
        );
        let solved = solved(&path);
        assert_eq!((solved.dof, solved.redundant), (7, 0), "{name}");
        // The relation as said, measured from the solved points of the reopened
        // file: from vector A to vector B, with their stated directions.
        let dir = |v: &Value| {
            let i = (0..4)
                .find(|&i| s.curve(i) == v["from"]["curve"])
                .expect("a Line");
            solved.dir(i, v["from"]["at"] == "start")
        };
        near(turn(dir(a), dir(b)), *degrees, name);
        // And the same relation read in stored order, the one fact all five say.
        near(solved.turn(1, 2), -70., name);
        let c = corners(&solved);
        assert!(
            c[1] < 0. && c[0] > 0. && c[2] > 0. && c[3] > 0.,
            "{name}: {c:?}"
        );
        assert!(solved.area > 0., "{name}: the profile kept its orientation");
        near(c.iter().sum::<f64>(), 360., name);
        stored_same(&s.path, &path);
        if first.is_none() {
            exports(&path, &solved, "dart");
            first = Some(path);
        }
    }
    // The mirror: +50 is a different shape, not the reflex one.
    let (path, result) = s.publish(
        &[angle(side(1, st, e), side(2, st, e), 50.)],
        "dart-mirror.fcad",
    );
    assert_eq!(result["constraints"][4]["rule"]["angle"]["value"], 50.0);
    let mirror = solved(&path);
    near(mirror.turn(1, 2), 50., "+50°");
    let c = corners(&mirror);
    assert!(
        c.iter().all(|t| *t > 0.),
        "+50° made a convex profile: {c:?}"
    );
    assert!(mirror.turn(1, 2) > 0. && stored[1] < 0.);
    exports(&path, &mirror, "dart-mirror");
    // The source did not move.
    assert_eq!(std::fs::read(&s.path).expect("source"), bytes);
    assert_eq!(flat_modified(&s.path), mtime);
}

/// Across the half turn: the opposite of a Line is `+180` only, and `+170` and
/// `-170` tilt the far side off antiparallel in opposite senses.
#[test]
fn native_directed_angle_across_the_half_turn_tilts_a_quadrilateral_either_way() {
    if !have(true, true) {
        return;
    }
    let s = Source::rectangle();
    let side = |i: usize| s.vector(i, "start", "end");
    let mut tilt = std::collections::BTreeMap::new();
    for (name, asked, published) in [
        ("+170", 170.0f64, 170.0f64),
        ("-170", -170., -170.),
        ("+180", 180., 180.),
        ("-180 is +180", -180., 180.),
        ("0", 0., 0.),
    ] {
        let out = s.root.path().join(format!("half-{name}.fcad"));
        let outcome = s
            .command(
                &s.path,
                &s.catalog,
                &[],
                &[angle(side(0), side(2), asked)],
                &out,
            )
            .output()
            .expect("publish");
        if asked == 0. {
            // Codirectional opposite sides cannot close a simple quadrilateral
            // from this profile: whatever the solver says, it is never a
            // published profile whose measured turn is anything but 0.
            if outcome.status.code() == Some(0) {
                let _v: Value = serde_json::from_slice(&outcome.stdout).expect("JSON");
                assert_eq!(
                    inspect(&out)["sketches"][0]["constraints"][4]["rule"]["angle"]["value"],
                    0.0
                );
                let solved = solved(&out);
                near(solved.turn(0, 2), 0., "0°");
            } else {
                assert_eq!(outcome.status.code(), Some(2), "{outcome:?}");
                assert!(!out.exists());
            }
            continue;
        }
        let _result = reply(outcome, OP, 0)["result"].clone();
        assert_eq!(
            inspect(&out)["sketches"][0]["constraints"][4]["rule"]["angle"]["value"],
            published,
            "{name}: the opposite is stored and reported only as +180"
        );
        assert_eq!(transaction::solve(&out)["degrees_of_freedom"], 7, "{name}");
        let solved = solved(&out);
        assert_eq!((solved.dof, solved.redundant), (7, 0), "{name}");
        let measured = solved.turn(0, 2);
        if published.abs() == 180. {
            assert!(
                measured.abs() > 180. - DEGREES,
                "{name}: measures {measured}°"
            );
        } else {
            near(measured, published, name);
        }
        tilt.insert(name, {
            let (u, v) = (solved.vectors[0], solved.vectors[2]);
            (u[0] * v[1] - u[1] * v[0]) / (u[0].hypot(u[1]) * v[0].hypot(v[1]))
        });
        stored_same(&s.path, &out);
        if published == 170. {
            exports(&out, &solved, "half-turn");
        }
    }
    // The sign is real geometry: the far side leans left for +170 and right for
    // -170, and neither leans for the opposite.
    assert!(tilt["+170"] > 0.17 && tilt["-170"] < -0.17, "{tilt:?}");
    assert!(
        tilt["+180"].abs() < 1e-6 && tilt["-180 is +180"].abs() < 1e-6,
        "{tilt:?}"
    );
}

/// Replacement and removal by exact UUID, the solver's own redundancy and
/// conflict with the UUIDs of their reasons, and publication that is all or
/// nothing — including the report lost after publication.
#[test]
fn native_directed_angle_replace_remove_diagnose_and_publish_atomically() {
    if !have(true, true) {
        return;
    }
    let s = Source::slanted();
    let (bytes, mtime) = (
        std::fs::read(&s.path).expect("source"),
        flat_modified(&s.path),
    );
    let turn_of = |i: usize, j: usize, degrees: f64| {
        angle(
            s.vector(i, "start", "end"),
            s.vector(j, "start", "end"),
            degrees,
        )
    };
    // Three right turns: a rectangle of free size, whose fourth corner follows.
    let three = [turn_of(0, 1, 90.), turn_of(1, 2, 90.), turn_of(2, 3, 90.)];
    let (rect, built) = s.publish(&three, "rect.fcad");
    assert_eq!(built["solve"]["degrees_of_freedom"], 5);
    assert_eq!(built["solve"]["redundant_constraint_ids"], json!([]));
    let kinds: Vec<_> = built["constraints"]
        .as_array()
        .expect("added")
        .iter()
        .map(|c| c["rule"]["kind"].as_str().expect("kind").to_owned())
        .collect();
    assert_eq!(
        kinds,
        [
            "coincident",
            "coincident",
            "coincident",
            "coincident",
            "directed_angle",
            "directed_angle",
            "directed_angle"
        ]
    );
    let grid = solved(&rect);
    for (i, t) in corners(&grid).iter().enumerate().take(3) {
        near(*t, 90., &format!("corner {i}"));
    }
    assert!(grid.area > 0.);
    stored_same(&s.path, &rect);
    exports(&rect, &grid, "rect");
    let rect_catalog = inspect(&rect);
    let rect_ids = constraint_ids(&rect_catalog);
    let angle_ids: Vec<Value> = built["constraints"].as_array().expect("angles")[4..]
        .iter()
        .map(|c| c["constraint_id"].clone())
        .collect();

    // A fourth right turn is implied by the other three: the real solver says
    // so, naming a UUID, and the profile still publishes.
    let four = [
        three[0].clone(),
        three[1].clone(),
        three[2].clone(),
        turn_of(3, 0, 90.),
    ];
    let (twice, repeated) = s.publish(&four, "rect-4.fcad");
    let said = repeated["solve"]["redundant_constraint_ids"]
        .as_array()
        .expect("ids");
    let minted: Vec<Value> = repeated["constraints"].as_array().expect("angles")[4..]
        .iter()
        .map(|c| c["constraint_id"].clone())
        .collect();
    assert_eq!(said.len(), 1, "{repeated}");
    assert!(
        minted.contains(&said[0]),
        "the redundancy names one of the four"
    );
    assert_eq!(repeated["solve"]["degrees_of_freedom"], 5);
    assert_eq!(solved(&twice).redundant, 1);

    // The same corner turned the other way cannot hold with the rest: a real
    // conflict, with typed reasons, nothing published.
    let wrong = [
        three[0].clone(),
        three[1].clone(),
        three[2].clone(),
        turn_of(3, 0, -90.),
    ];
    let error = s.refused(&s.path, &s.catalog, &wrong);
    assert_eq!(error["kind"], "constraint", "{error}");
    let blamed = error["constraint_conflict"]["constraints"]
        .as_array()
        .expect("typed conflict");
    assert!(!blamed.is_empty());
    for c in blamed {
        assert!(c["constraint_id"].is_string(), "{c}");
        assert_eq!(c["rule"]["kind"], "directed_angle", "{c}");
        assert!(
            c["rule"]["angle"]["value"] == 90.0 || c["rule"]["angle"]["value"] == -90.0,
            "{c}"
        );
    }

    // Replacing the middle corner: the exact UUID off and a new angle on, in one
    // request. Every other UUID, rule and order survives; the shape follows.
    let replaced_out = s.root.path().join("rect-60.fcad");
    let _replaced = reply(
        s.command(
            &rect,
            &rect_catalog,
            &[angle_ids[1].clone()],
            &[turn_of(1, 2, 60.)],
            &replaced_out,
        )
        .output()
        .expect("replace"),
        OP,
        0,
    )["result"]
        .clone();

    assert_eq!(transaction::solve(&replaced_out)["degrees_of_freedom"], 5);
    let after = inspect(&replaced_out);
    let kept: Vec<_> = rect_catalog["sketches"][0]["constraints"]
        .as_array()
        .expect("constraints")
        .iter()
        .filter(|c| c["constraint_id"] != angle_ids[1])
        .cloned()
        .collect();
    assert_eq!(
        after["sketches"][0]["constraints"],
        json!(
            [
                kept,
                after["sketches"][0]["constraints"]
                    .as_array()
                    .expect("constraints")
                    .iter()
                    .filter(|c| !rect_catalog["sketches"][0]["constraints"]
                        .as_array()
                        .expect("old constraints")
                        .iter()
                        .any(|old| old["constraint_id"] == c["constraint_id"]))
                    .cloned()
                    .collect()
            ]
            .concat()
        ),
        "every other UUID, rule and order survives"
    );
    let trapezoid = solved(&replaced_out);
    near(trapezoid.turn(1, 2), 60., "replaced corner");
    near(trapezoid.turn(0, 1), 90., "kept corner");
    near(trapezoid.turn(2, 3), 90., "kept corner");
    near(trapezoid.turn(3, 0), 120., "the corner that follows");
    stored_same(&rect, &replaced_out);

    // Removing every angle by UUID gives the freedom back, and only the joints
    // remain.
    let free_out = s.root.path().join("rect-free.fcad");
    let _freed = reply(
        s.command(&rect, &rect_catalog, &angle_ids, &[], &free_out)
            .output()
            .expect("remove"),
        OP,
        0,
    )["result"]
        .clone();

    assert_eq!(transaction::solve(&free_out)["degrees_of_freedom"], 8);
    assert_eq!(
        constraint_ids(&inspect(&free_out)),
        rect_ids[..4].to_vec(),
        "the four closure joints and nothing else"
    );
    assert_eq!(solved(&free_out).dof, 8);

    // A stale snapshot, an occupied destination and a lost report.
    let stale = s
        .command(
            &rect,
            &s.catalog,
            &[],
            &[turn_of(0, 2, 180.)],
            &s.root.path().join("stale.fcad"),
        )
        .output()
        .expect("stale");
    assert_eq!(stale.status.code(), Some(2), "{stale:?}");
    assert!(!s.root.path().join("stale.fcad").exists());
    let taken = s.root.path().join("taken.fcad");
    std::fs::write(&taken, b"somebody else's file").expect("taken");
    let o = s
        .command(&s.path, &s.catalog, &[], &[turn_of(0, 1, 90.)], &taken)
        .output()
        .expect("taken");
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    assert_eq!(
        std::fs::read(&taken).expect("taken"),
        b"somebody else's file"
    );
    for both in [false, true] {
        let out = s.root.path().join(format!("lost-{both}.fcad"));
        let mut c = s.command(&s.path, &s.catalog, &[], &[turn_of(0, 1, 90.)], &out);
        c.stdout(pipe::closed_pipe());
        if both {
            c.stderr(pipe::closed_pipe());
        }
        let o = c.output().expect("lost report");
        assert_eq!(o.status.code(), Some(7), "{o:?}");
        stored_same(&s.path, &out);
        let listed = inspect(&out);
        assert_eq!(constraint_ids(&listed).len(), 5, "published once, whole");
        assert_eq!(
            listed["sketches"][0]["constraints"][4]["rule"]["angle"]["value"],
            90.0
        );
        assert_eq!(solved(&out).dof, 7);
    }
    // The source is the source after all of it.
    assert_eq!(std::fs::read(&s.path).expect("source"), bytes);
    assert_eq!(flat_modified(&s.path), mtime);
    assert!(
        !entries(s.root.path())
            .iter()
            .any(|n| n.to_string_lossy().starts_with(".ferritecad-")),
        "a temporary file was left behind"
    );
}

/// A warm rebuild never serves one angle's solid for another's: the cache key
/// follows the solved profile, not the stored guess, which is the same in both.
#[test]
fn native_directed_angle_cache_keys_follow_the_solved_profile() {
    if !have(true, true) {
        return;
    }
    let s = Source::dart();
    let side = |i: usize| s.vector(i, "start", "end");
    let (reflex, _) = s.publish(&[angle(side(1), side(2), -70.)], "cache-a.fcad");
    let (convex, _) = s.publish(&[angle(side(1), side(2), 50.)], "cache-b.fcad");
    let sidecar = s.root.path().join("shared.fcad-cache");
    let volume = |path: &Path| -> (CacheOutcome, f64) {
        let d = Document::open_read_only(path).expect("open");
        let extrude = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Extrude(_)))
            .expect("extrude")
            .id;
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        let mut cache = CacheStore::open(
            &sidecar,
            d.meta().document_id,
            kernel.identity().id(),
            kernel.identity().version(),
        )
        .expect("sidecar");
        let (built, events) =
            rebuild_cached(&d, &mut kernel, &mut cache, &OperationContext::default())
                .expect("a cached rebuild");
        assert!(
            events
                .iter()
                .all(|e| e.outcome != CacheOutcome::WriteFailed)
        );
        let shape = built.shape(extrude).expect("a solid");
        let (_, volume) = kernel.shape_stats(shape).expect("stats");
        let outcome = events.last().expect("an event").outcome;
        built.release_all(&mut kernel);
        d.close().expect("close");
        (outcome, volume)
    };
    let (first, v_reflex) = volume(&reflex);
    assert_eq!(first, CacheOutcome::Miss);
    // Same document, same stored curves, same sidecar: a different angle.
    let (second, v_convex) = volume(&convex);
    assert_eq!(second, CacheOutcome::Miss, "a stale solid was served");
    assert!(
        (v_reflex - v_convex).abs() > 1000.,
        "{v_reflex} vs {v_convex}"
    );
    for (path, expected) in [(&reflex, v_reflex), (&convex, v_convex)] {
        let (outcome, again) = volume(path);
        assert_eq!(outcome, CacheOutcome::Hit);
        assert!((again - expected).abs() < 1e-9, "{again} vs {expected}");
        let independent = solved(path).area.abs() * 10.;
        assert!(
            (again - independent).abs() < independent * 1e-6,
            "{again} vs {independent}"
        );
    }
}

fn facts(path: &Path) -> Value {
    let listed = inspect(path);
    json!({"constraints":listed["sketches"][0]["constraints"],"solve":transaction::solve(path)})
}
