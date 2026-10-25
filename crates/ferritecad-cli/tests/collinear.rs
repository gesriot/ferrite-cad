// SPDX-License-Identifier: MIT
//! §31G: two saved Lines on one infinite line, through the shipped command
//! `edit`.
//!
//! Everything asserted on a solved profile is measured here, from the solved
//! points of the published file after it was reopened and rebuilt cold: the
//! signed distance of each end of each Line from the infinite line through the
//! other, the sine of the angle between them and their lengths, written out
//! again below and never read back from the request, the stored constraint or
//! the solver. The profiles are chosen so that a Parallel alone would pass the
//! direction and fail the offset: a U whose two rims are parallel, disjoint and
//! 2 mm apart, the same U turned off the axes, and a Z whose rims run in
//! opposite directions.
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

const OP: &str = "edit";
/// Solved distances (mm) and sines agree with the request to well inside this.
const TOL: f64 = 1e-6;

/// Two parallel rims 2 mm apart on a U (Lines 0 and 4), right one on top.
const U: [[f64; 2]; 8] = [
    [50., 22.],
    [35., 22.],
    [35., 8.],
    [15., 8.],
    [15., 20.],
    [0., 20.],
    [0., 0.],
    [50., 0.],
];
/// The ledges of a Z (Lines 2 and 6) run in opposite directions, 1 mm apart.
const Z: [[f64; 2]; 8] = [
    [0., 0.],
    [40., 0.],
    [40., 9.],
    [60., 9.],
    [60., 21.],
    [20., 21.],
    [20., 10.],
    [0., 10.],
];
/// The same Z with its ledges already on one line (y = 10).
const Z_FLAT: [[f64; 2]; 8] = [
    [0., 0.],
    [40., 0.],
    [40., 10.],
    [60., 10.],
    [60., 21.],
    [20., 21.],
    [20., 10.],
    [0., 10.],
];

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
    /// The U turned about the origin, so that no rim is on an axis.
    fn inclined(degrees: f64) -> (Self, [[f64; 2]; 8]) {
        let (s, c) = degrees.to_radians().sin_cos();
        let turned = U.map(|[x, y]| [x * c - y * s, x * s + y * c]);
        (Self::new(&turned), turned)
    }
    fn curve(&self, i: usize) -> Value {
        self.catalog["sketches"][0]["curves"][i]["curve_id"].clone()
    }
    fn collinear(&self, i: usize, j: usize) -> Value {
        self.rule("collinear", i, j)
    }
    fn rule(&self, rule: &str, i: usize, j: usize) -> Value {
        transaction::relation(&self.curve(i), &self.curve(j), rule)
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

/// One published file, reopened and rebuilt cold.
struct Solved {
    /// Line `i` as `[start, end]`, solved.
    lines: Vec<[[f64; 2]; 2]>,
    dof: usize,
    redundant: usize,
    area: f64,
}
/// What two Lines are to each other, from solved points alone.
struct Gap {
    /// Signed distance, positive to the left of the other Line as it runs:
    /// A's start and end from B's line, then B's start and end from A's line.
    off: [f64; 4],
    /// The sine of the angle from A to B.
    sine: f64,
    lengths: [f64; 2],
}
impl Gap {
    fn worst(&self) -> f64 {
        self.off.iter().fold(self.sine.abs(), |w, d| w.max(d.abs()))
    }
}
impl Solved {
    fn gap(&self, i: usize, j: usize) -> Gap {
        let [a0, a1] = self.lines[i];
        let [b0, b1] = self.lines[j];
        let (da, db) = (
            [a1[0] - a0[0], a1[1] - a0[1]],
            [b1[0] - b0[0], b1[1] - b0[1]],
        );
        let (la, lb) = (da[0].hypot(da[1]), db[0].hypot(db[1]));
        let side = |p: [f64; 2], from: [f64; 2], d: [f64; 2], l: f64| {
            (d[0] * (p[1] - from[1]) - d[1] * (p[0] - from[0])) / l
        };
        Gap {
            off: [
                side(a0, b0, db, lb),
                side(a1, b0, db, lb),
                side(b0, a0, da, la),
                side(b1, a0, da, la),
            ],
            sine: (da[0] * db[1] - da[1] * db[0]) / (la * lb),
            lengths: [la, lb],
        }
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
    // A Sketch with no constraints is not solved and has no report.
    let (dof, redundant) = built.solve_report(sketch.id).map_or((0, 0), |report| {
        (report.degrees_of_freedom(), report.redundant().len())
    });
    let presentation = built.sketch_presentation(sketch.id).expect("presentation");
    let mut lines = vec![];
    for (curve, saved) in presentation.curves().iter().zip(&stored.curves) {
        assert_eq!(curve.id(), saved.id, "the solve kept every UUID and order");
        let SketchGeometry::Line { start, end } = curve.geometry() else {
            panic!("a Line")
        };
        lines.push([[start.x, start.y], [end.x, end.y]]);
    }
    for (i, line) in lines.iter().enumerate() {
        let next = lines[(i + 1) % lines.len()][0];
        assert!(
            (line[1][0] - next[0]).abs() < 1e-6 && (line[1][1] - next[1]).abs() < 1e-6,
            "joint {i} is open: {:?} {next:?}",
            line[1]
        );
    }
    crate::names::assert_primary(&built);
    built.release_all(&mut kernel);
    d.close().expect("close");
    for (i, line) in lines.iter().enumerate() {
        assert!(
            (line[1][0] - line[0][0]).hypot(line[1][1] - line[0][1]) > 1.,
            "Line {i} collapsed: {line:?}"
        );
    }
    let area = lines
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let b = lines[(i + 1) % lines.len()][0];
            a[0][0] * b[1] - a[0][1] * b[0]
        })
        .sum::<f64>()
        / 2.;
    Solved {
        lines,
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
    if let Some(dir) = std::env::var_os("FCAD_COLLINEAR_ARTIFACT_DIR") {
        std::fs::create_dir_all(&dir).expect("artifacts");
        std::fs::copy(&fbx, Path::new(&dir).join(format!("collinear-{tag}.fbx")))
            .expect("artifact");
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

/// What a published copy must still be, whatever it carries: the source's
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
fn flat_modified(path: &Path) -> std::time::SystemTime {
    std::fs::metadata(path)
        .expect("meta")
        .modified()
        .expect("mtime")
}
fn near(x: f64, y: f64, what: &str) {
    assert!((x - y).abs() < TOL, "{what}: {x} is not {y}");
}
/// The relation holds: every distance and the sine are zero, and neither Line
/// has collapsed. Measured, not read back.
fn on_one_line(solved: &Solved, i: usize, j: usize, what: &str) {
    let gap = solved.gap(i, j);
    for d in gap.off {
        near(d, 0., &format!("{what}: an end is off the other's line"));
    }
    near(gap.sine, 0., &format!("{what}: the sine"));
    assert!(
        gap.lengths.iter().all(|l| *l > 5.),
        "{what}: {:?}",
        gap.lengths
    );
}

/// Discovery: a stored collinearity is reported by its own kind and both Lines,
/// the older relations exactly as before, and the new capability and layout are
/// in the file. The relation is written through the document, so neither a
/// kernel nor a solver is needed to read it back.
#[test]
fn discovery_names_a_stored_collinear_and_both_lines_and_leaves_the_older_forms_alone() {
    use ferritecad_document::{
        AddLineConstraint, AddSketchConstraint, LineRelation, SketchConstraintEdits,
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
    let relation = |a: usize, b: usize, relation| {
        AddSketchConstraint::Line(AddLineConstraint::Relation {
            a: ids[a],
            b: ids[b],
            relation,
        })
    };
    let prepared = ferritecad_fixtures::constraint_changes(
        &d,
        sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![
                relation(2, 0, LineRelation::Collinear),
                relation(1, 3, LineRelation::Parallel),
            ],
        },
    )
    .expect("prepared");
    let (collinear_id, parallel_id) = (
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
        by_id(&collinear_id),
        json!({"kind":"collinear",
               "a":{"from":end(2,"start"),"to":end(2,"end")},
               "b":{"from":end(0,"start"),"to":end(0,"end")}}),
        "its own kind, both Lines whole, in the order asked, and no number"
    );
    // The older relation keeps exactly the shape it always had.
    assert_eq!(
        by_id(&parallel_id),
        json!({"kind":"parallel",
               "a":{"from":end(1,"start"),"to":end(1,"end")},
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
    // The same sketch without the collinearity is the layout every older build
    // writes.
    let mut d = Document::open(&with).expect("reopen");
    let removal = ferritecad_fixtures::constraint_changes(
        &d,
        sketch,
        &SketchConstraintEdits {
            remove: vec![collinear_id],
            add: vec![],
        },
    )
    .expect("remove");
    d.write_changes(&removal).expect("write");
    d.close().expect("close");
}

/// OCCT without a solver: a document that holds a collinearity is neither
/// recomputed nor published as solved, and the older route (an unconstrained
/// document, its coordinates and its exports) works exactly as before.
#[test]
fn without_a_solver_a_collinear_document_is_not_solved_and_the_unconstrained_route_works() {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")])
        || ferritecad_eval::solver_available()
    {
        return;
    }
    use ferritecad_document::{
        AddLineConstraint, AddSketchConstraint, LineRelation, SketchConstraintEdits,
    };
    let s = Source::new(&U);
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
    // The same request on the unconstrained document, which the build refuses
    // for want of a solver and publishes nothing.
    let error = s.refused(&s.path, &s.catalog, &[s.collinear(0, 4)]);
    assert_eq!(error["kind"], "unsupported", "{error}");
    // A collinearity stored through the document.
    let with = s.root.path().join("collinear.fcad");
    std::fs::copy(&s.path, &with).expect("copy");
    let mut d = Document::open(&with).expect("open");
    let sketch = d
        .objects()
        .expect("objects")
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("sketch")
        .id;
    let ids: Vec<ferritecad_types::StableEntityId> = [0, 4]
        .iter()
        .map(|&i| s.curve(i).as_str().expect("id").parse().expect("UUID"))
        .collect();
    let prepared = ferritecad_fixtures::constraint_changes(
        &d,
        sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![AddSketchConstraint::Line(AddLineConstraint::Relation {
                a: ids[0],
                b: ids[1],
                relation: LineRelation::Collinear,
            })],
        },
    )
    .expect("prepared");
    d.write_changes(&prepared).expect("written");
    d.close().expect("close");
    // Nothing solves it and nothing is published as if it had been.
    let stl = s.root.path().join("collinear.stl");
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
    let error = s.refused(&with, &catalog, &[s.rule("equal_length", 1, 2)]);
    assert_eq!(error["kind"], "unsupported", "{error}");
}

/// The U's two rims are parallel, apart and 2 mm off one another's line. The
/// collinearity brings them onto one line; a Parallel, which passes the
/// direction and nothing else, leaves them where they were. Both are measured
/// from the reopened file, and the unsolved source says which way is which.
#[test]
fn native_collinear_brings_disjoint_parallel_rims_onto_one_line_and_a_parallel_does_not() {
    if !have(true, true) {
        return;
    }
    let s = Source::new(&U);
    let (bytes, mtime) = (
        std::fs::read(&s.path).expect("source"),
        flat_modified(&s.path),
    );
    // The signs and the scale of the measurement, on the stored coordinates
    // alone: rim 0 runs west at y = 22, rim 4 runs west at y = 20, so rim 0's
    // ends lie 2 to the right of rim 4's line and rim 4's 2 to the left of rim
    // 0's. A measurement that cannot say this says nothing about the solve.
    let start = solved(&s.path);
    let gap = start.gap(0, 4);
    for (got, want) in gap.off.iter().zip([-2., -2., 2., 2.]) {
        near(*got, want, "the stored offsets");
    }
    near(gap.sine, 0., "the stored rims are parallel");
    near(
        start.gap(4, 0).off[0],
        2.,
        "the same pair the other way round",
    );
    assert_eq!(
        (start.dof, start.redundant),
        (0, 0),
        "an unconstrained Sketch is not solved"
    );

    let (path, result) = s.publish(&[s.collinear(0, 4)], "u-collinear.fcad");
    let added = result["constraints"].as_array().expect("added");
    assert_eq!(added.len(), 9, "eight joints and the collinearity");
    assert_eq!(
        added[8]["rule"],
        json!({"kind":"collinear",
               "a":{"from":{"curve_id":s.curve(0),"at":"start"},"to":{"curve_id":s.curve(0),"at":"end"}},
               "b":{"from":{"curve_id":s.curve(4),"at":"start"},"to":{"curve_id":s.curve(4),"at":"end"}}}),
        "published exactly as asked"
    );
    assert_eq!(result["solve"]["degrees_of_freedom"], 14);
    assert_eq!(result["solve"]["redundant_constraint_ids"], json!([]));
    let together = solved(&path);
    assert_eq!((together.dof, together.redundant), (14, 0));
    on_one_line(&together, 0, 4, "rims");
    // The other way round is the same relation.
    on_one_line(&together, 4, 0, "rims, the other way round");
    // Neither rim was dragged out of its place: they met between their stored
    // heights, each a rim of about the length it was.
    for rim in [0, 4] {
        let y = (together.lines[rim][0][1] + together.lines[rim][1][1]) / 2.;
        assert!((20. ..=22.).contains(&y), "rim {rim} is at y = {y}");
    }
    assert!(together.area > 0.);
    stored_same(&s.path, &path);
    exports(&path, &together, "u");

    // The negative control: a Parallel is held and the offset is not.
    let (control, parallel) = s.publish(&[s.rule("parallel", 0, 4)], "u-parallel.fcad");
    assert_eq!(parallel["solve"]["degrees_of_freedom"], 15);
    let apart = solved(&control);
    near(apart.gap(0, 4).sine, 0., "the Parallel holds");
    for (got, want) in apart.gap(0, 4).off.iter().zip([-2., -2., 2., 2.]) {
        near(*got, want, "a Parallel moved the rims");
    }
    assert!(apart.gap(0, 4).worst() > 1.9);
    exports(&control, &apart, "u-parallel");

    // The source did not move.
    assert_eq!(std::fs::read(&s.path).expect("source"), bytes);
    assert_eq!(flat_modified(&s.path), mtime);
}

/// Off the axes, and with Lines running opposite ways: the same relation. The
/// U turned by 23° has no rim on an axis, and the Z's ledges run in opposite
/// directions with the profile traversed the one way.
#[test]
fn native_collinear_holds_on_a_turned_u_and_between_lines_that_run_opposite_ways() {
    if !have(true, true) {
        return;
    }
    // The turned U: 2 mm perpendicular to the rims before, nothing after.
    let (s, turned) = Source::inclined(23.);
    let before = solved(&s.path);
    let gap = before.gap(0, 4);
    for (got, want) in gap.off.iter().zip([-2., -2., 2., 2.]) {
        near(*got, want, "the turned rims' stored offsets");
    }
    assert!(
        turned[0][0] != turned[1][0] && turned[0][1] != turned[1][1],
        "the rim is on an axis"
    );
    let (path, result) = s.publish(&[s.collinear(0, 4)], "turned.fcad");
    assert_eq!(result["solve"]["degrees_of_freedom"], 14);
    let together = solved(&path);
    on_one_line(&together, 0, 4, "turned rims");
    exports(&path, &together, "turned");
    let (control, _) = s.publish(&[s.rule("parallel", 0, 4)], "turned-parallel.fcad");
    let apart = solved(&control);
    near(apart.gap(0, 4).sine, 0., "the turned Parallel holds");
    assert!(apart.gap(0, 4).worst() > 1.9, "{:?}", apart.gap(0, 4).off);

    // The Z: Line 2 runs east at y = 9, Line 6 runs west at y = 10.
    let z = Source::new(&Z);
    let start = solved(&z.path);
    let (a, b) = (start.lines[2], start.lines[6]);
    assert!(
        a[1][0] > a[0][0] && b[1][0] < b[0][0],
        "the ledges run opposite ways: {a:?} {b:?}"
    );
    // Line 6 is north of Line 2's eastward line (on its left), and Line 2 is
    // south of Line 6's westward line (on its left too).
    for (got, want) in start.gap(2, 6).off.iter().zip([1., 1., 1., 1.]) {
        near(*got, want, "the Z ledges' stored offsets");
    }
    // Either Line leading is the same relation.
    for (name, i, j) in [("2 then 6", 2, 6), ("6 then 2", 6, 2)] {
        let (path, result) = z.publish(&[z.collinear(i, j)], &format!("z-{i}-{j}.fcad"));
        assert_eq!(result["solve"]["degrees_of_freedom"], 14, "{name}");
        let together = solved(&path);
        on_one_line(&together, 2, 6, name);
        // Still opposite: collinear holds the line, not a direction along it.
        let (a, b) = (together.lines[2], together.lines[6]);
        assert!(
            a[1][0] > a[0][0] && b[1][0] < b[0][0],
            "{name}: {a:?} {b:?}"
        );
        assert!(together.area > 0.);
        stored_same(&z.path, &path);
        if i == 2 {
            exports(&path, &together, "z");
        }
    }
}

/// Replacement and removal by exact UUID, the solver's own conflict and
/// redundancy with the UUIDs of their reasons, and the profile rules that stay
/// the last word: what would fold the profile onto itself is not published.
#[test]
fn native_collinear_replace_remove_diagnose_and_leave_the_profile_rules_the_last_word() {
    if !have(true, true) {
        return;
    }
    let s = Source::new(&U);
    let (together, built) = s.publish(&[s.collinear(0, 4)], "u-collinear.fcad");
    let collinear_id = built["constraints"][8]["constraint_id"].clone();
    let together_catalog = inspect(&together);
    let together_ids = constraint_ids(&together_catalog);

    // A Parallel in its place: the exact UUID off and the other answer on, in
    // one request. Every other UUID, rule and order survives, and the layout is
    // the older one again.
    let replaced_out = s.root.path().join("u-parallel.fcad");
    let _replaced = reply(
        s.command(
            &together,
            &together_catalog,
            std::slice::from_ref(&collinear_id),
            &[s.rule("parallel", 0, 4)],
            &replaced_out,
        )
        .output()
        .expect("replace"),
        OP,
        0,
    )["result"]
        .clone();

    assert_eq!(transaction::solve(&replaced_out)["degrees_of_freedom"], 15);
    let after = inspect(&replaced_out);
    let first_eight = |catalog: &Value| {
        catalog["sketches"][0]["constraints"]
            .as_array()
            .expect("constraints")[..8]
            .to_vec()
    };
    assert_eq!(
        first_eight(&after),
        first_eight(&together_catalog),
        "the joints keep their UUIDs, rules and order"
    );
    assert!(!constraint_ids(&after).contains(&collinear_id));
    let apart = solved(&replaced_out);
    near(apart.gap(0, 4).sine, 0., "the Parallel holds");
    assert!(apart.gap(0, 4).worst() > 1.9, "no longer on one line");

    stored_same(&together, &replaced_out);

    // Removing it gives the freedom back; only the joints remain.
    let free_out = s.root.path().join("u-free.fcad");
    let _freed = reply(
        s.command(
            &together,
            &together_catalog,
            std::slice::from_ref(&collinear_id),
            &[],
            &free_out,
        )
        .output()
        .expect("remove"),
        OP,
        0,
    )["result"]
        .clone();

    assert_eq!(transaction::solve(&free_out)["degrees_of_freedom"], 16);
    assert_eq!(
        constraint_ids(&inspect(&free_out)),
        together_ids[..8].to_vec(),
        "the eight closure joints and nothing else"
    );
    assert_eq!(solved(&free_out).dof, 16);

    // A conflict: both rims are held horizontal at the heights they have, by a
    // pin, a length and a wall each, so no line holds both. Without the
    // collinearity that profile is solved; with it the reasons are typed,
    // include the collinearity, and nothing is published.
    let held = [
        transaction::fixed(&s.curve(6), "start", 0 as f64, 0 as f64),
        transaction::line_rule(&s.curve(6), "horizontal"),
        transaction::line_rule(&s.curve(5), "vertical"),
        transaction::distance(&s.curve(5), 20_f64),
        transaction::line_rule(&s.curve(4), "horizontal"),
        transaction::line_rule(&s.curve(7), "vertical"),
        transaction::distance(&s.curve(7), 22_f64),
        transaction::line_rule(&s.curve(0), "horizontal"),
    ];
    let (pinned, _) = s.publish(&held, "u-pinned.fcad");
    let pinned_catalog = inspect(&pinned);
    let pinned_state = solved(&pinned);
    near(
        pinned_state.gap(0, 4).off[0],
        -2.,
        "the pinned rims are 2 apart",
    );
    let error = s.refused(&pinned, &pinned_catalog, &[s.collinear(0, 4)]);
    assert_eq!(error["kind"], "constraint", "{error}");
    let reasons = error["constraint_conflict"]["constraints"]
        .as_array()
        .expect("typed reasons");
    assert!(
        reasons.iter().all(|c| c["constraint_id"].is_string()),
        "{reasons:?}"
    );
    let named: Vec<_> = reasons.iter().map(|c| c["rule"]["kind"].clone()).collect();
    assert!(named.contains(&json!("collinear")), "{named:?}");
    let blamed = reasons
        .iter()
        .find(|c| c["rule"]["kind"] == "collinear")
        .expect("the collinearity is blamed");
    assert_eq!(blamed["rule"]["a"]["from"]["curve_id"], s.curve(0));
    assert_eq!(blamed["rule"]["b"]["from"]["curve_id"], s.curve(4));
    for kind in named {
        assert!(
            [
                "collinear",
                "horizontal",
                "vertical",
                "distance",
                "fixed",
                "coincident"
            ]
            .contains(&kind.as_str().expect("a kind")),
            "{kind}"
        );
    }

    // A redundancy: two ledges that already lie on one line, each told to be
    // horizontal as well as collinear. Three independent facts are said by four
    // equations, so the real solver names one of the constraints, publishes,
    // and counts the freedom once.
    let flat = Source::new(&Z_FLAT);
    let start = solved(&flat.path);
    for d in start.gap(2, 6).off {
        near(d, 0., "the flat Z's ledges are already on one line");
    }
    let said = [
        transaction::line_rule(&flat.curve(2), "horizontal"),
        transaction::line_rule(&flat.curve(6), "horizontal"),
        flat.collinear(2, 6),
    ];
    let (twice, repeated) = flat.publish(&said, "z-flat.fcad");
    let minted: Vec<Value> = repeated["constraints"].as_array().expect("added")[8..]
        .iter()
        .map(|c| c["constraint_id"].clone())
        .collect();
    let redundant = repeated["solve"]["redundant_constraint_ids"]
        .as_array()
        .expect("ids");
    assert_eq!(redundant.len(), 1, "{repeated}");
    assert!(minted.contains(&redundant[0]), "it names one of the three");
    assert_eq!(repeated["solve"]["degrees_of_freedom"], 13);
    let state = solved(&twice);
    assert_eq!((state.dof, state.redundant), (13, 1));
    on_one_line(&state, 2, 6, "flat Z");

    // The evaluator accepts a straight corner; the retired preparer's corner policy is gone.
    let (straight, _) = s.publish(&[s.collinear(0, 1)], "straight-corner.fcad");
    let straight_state = solved(&straight);
    let straight_gap = straight_state.gap(0, 1);
    for offset in straight_gap.off {
        near(offset, 0., "straight corner offset");
    }
    near(straight_gap.sine, 0., "straight corner sine");
    assert!(straight_gap.lengths.iter().all(|length| *length > TOL));
    stored_same(&s.path, &straight);
    exports(&straight, &straight_state, "straight-corner");
    let (aligned, _) = s.publish(&[s.collinear(2, 6)], "aligned-sides.fcad");
    let aligned_state = solved(&aligned);
    on_one_line(&aligned_state, 2, 6, "aligned sides");
    stored_same(&s.path, &aligned);
    exports(&aligned, &aligned_state, "aligned-sides");
    // A snapshot older than the file is refused, and nothing is published.
    let stale = s
        .command(
            &together,
            &s.catalog,
            &[],
            &[s.rule("equal_length", 0, 4)],
            &s.root.path().join("stale.fcad"),
        )
        .output()
        .expect("stale");
    assert_eq!(stale.status.code(), Some(2), "{stale:?}");
    assert!(!s.root.path().join("stale.fcad").exists());
}

/// A warm rebuild never serves one profile's solid for another's: the cache key
/// follows the solved profile, not the stored guess, which is the same in both.
#[test]
fn native_collinear_cache_keys_follow_the_solved_profile() {
    if !have(true, true) {
        return;
    }
    let s = Source::new(&U);
    let (together, _) = s.publish(&[s.collinear(0, 4)], "cache-a.fcad");
    let (apart, _) = s.publish(&[s.rule("parallel", 0, 4)], "cache-b.fcad");
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
    let (first, v_together) = volume(&together);
    assert_eq!(first, CacheOutcome::Miss);
    // Same document, same stored curves, same sidecar: a different profile.
    let (second, v_apart) = volume(&apart);
    assert_eq!(second, CacheOutcome::Miss, "a stale solid was served");
    assert!(
        (v_together - v_apart).abs() > 1.,
        "{v_together} vs {v_apart}"
    );
    for (path, expected) in [(&together, v_together), (&apart, v_apart)] {
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
