// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
use ferritecad_document::*;
use ferritecad_kernel::OperationContext;
use ferritecad_types::{ObjectId, StableEntityId};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};
#[path = "support/pipe.rs"]
mod pipe;
#[path = "support/transaction.rs"]
mod transaction;
const OP: &str = "edit";
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

fn native() -> bool {
    ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (cfg!(feature = "planegcs"), "PLANEGCS"),
    ])
}

fn stored_same(a: &Path, b: &Path) {
    let a = Document::open_read_only(a).expect("source");
    let b = Document::open_read_only(b).expect("copy");
    let mut meta = a.meta().clone();
    meta.modified_at = b.meta().modified_at.clone();
    assert_eq!(&meta, b.meta());
    assert_eq!(
        a.dependencies().expect("deps"),
        b.dependencies().expect("deps")
    );
    assert_eq!(
        a.topology_refs().expect("refs"),
        b.topology_refs().expect("refs")
    );
    for o in a.objects().expect("objects") {
        let n = b.object(o.id).expect("read").expect("same ID");
        if let (ObjectPayload::Sketch(s), ObjectPayload::Sketch(t)) = (&o.payload, &n.payload) {
            assert_eq!(s.plane, t.plane);
            assert_eq!(s.curves, t.curves);
            assert_eq!(o.name, n.name);
            assert_eq!(o.parent, n.parent);
            assert_eq!(o.ordinal, n.ordinal);
        } else {
            assert_eq!(o, n);
        }
    }
    assert!(b.validate().expect("validate").is_ok());
    a.close().expect("close");
    b.close().expect("close");
}

fn geometry_checked(
    path: &Path,
    dof: Option<usize>,
    check: impl FnOnce(&[[f64; 2]], &[[f64; 2]], &ferritecad_document::Sketch),
) {
    let d = Document::open_read_only(path).expect("doc");
    let objects = d.objects().expect("objects");
    let o = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .expect("sketch");
    let ObjectPayload::Sketch(s) = &o.payload else {
        panic!("sketch")
    };
    let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
    let b = ferritecad_eval::rebuild_cold(&d, &mut k, &OperationContext::default())
        .expect("cold after reopen");
    let report = b.solve_report(o.id).expect("solve report");
    match dof {
        Some(n) => assert_eq!(report.degrees_of_freedom(), n, "cold reopen solved DOF"),
        None => assert!(report.degrees_of_freedom() > 0),
    }
    assert!(report.redundant().is_empty());
    let p = b.sketch_presentation(o.id).expect("solved presentation");
    let mut starts = vec![];
    let mut ends = vec![];
    for (curve, stored) in p.curves().iter().zip(&s.curves) {
        assert_eq!(curve.id(), stored.id);
        let SketchGeometry::Line { start, end } = curve.geometry() else {
            panic!("line")
        };
        starts.push([start.x, start.y]);
        ends.push([end.x, end.y]);
    }
    for (i, end) in ends.iter().enumerate() {
        let next = starts[(i + 1) % starts.len()];
        assert!(
            (end[0] - next[0]).abs() < 1e-6 && (end[1] - next[1]).abs() < 1e-6,
            "joint {i}: {end:?} != {next:?}"
        );
    }
    check(&starts, &ends, s);
    let area: f64 = starts
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let z = starts[(i + 1) % starts.len()];
            a[0] * z[1] - a[1] * z[0]
        })
        .sum::<f64>()
        .abs()
        / 2.;
    let lo = [
        starts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
        starts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
        0.,
    ];
    let hi = [
        starts
            .iter()
            .map(|p| p[0])
            .fold(f64::NEG_INFINITY, f64::max),
        starts
            .iter()
            .map(|p| p[1])
            .fold(f64::NEG_INFINITY, f64::max),
        10.,
    ];
    for r in d.topology_refs().expect("refs") {
        assert_eq!(b.resolve(&r).expect("same topology ref").len(), 1);
    }
    b.release_all(&mut k);
    d.close().expect("close");
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
    assert_eq!(v["result"]["bytes"], bytes.len());
    let mut actual_lo = [f64::INFINITY; 3];
    let mut actual_hi = [f64::NEG_INFINITY; 3];
    let mut volume6 = 0.;
    for t in bytes[84..].chunks_exact(50) {
        let mut p = [[0.; 3]; 3];
        for (i, v) in p.iter_mut().enumerate() {
            for j in 0..3 {
                let at = 12 + 12 * i + 4 * j;
                v[j] = f64::from(f32::from_le_bytes(
                    t[at..at + 4].try_into().expect("coordinate"),
                ));
                actual_lo[j] = actual_lo[j].min(v[j]);
                actual_hi[j] = actual_hi[j].max(v[j]);
            }
        }
        let [a, b, c] = p;
        volume6 += a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
    }
    for i in 0..3 {
        assert!((lo[i] - actual_lo[i]).abs() < 1e-4);
        assert!((hi[i] - actual_hi[i]).abs() < 1e-4);
    }
    assert!(
        (volume6.abs() / 6. - area * 10.).abs() < area * 10. * 1e-6,
        "independent volume disagrees with solved polygon"
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
    if let Some(root) = std::env::var_os("FERRITECAD_CONSTRAINT_ARTIFACTS") {
        std::fs::create_dir_all(&root).expect("artifacts");
        for p in [path, &stl, &fbx] {
            std::fs::copy(p, Path::new(&root).join(p.file_name().expect("name")))
                .expect("artifact");
        }
    }
}

fn geometry(path: &Path, horizontal: bool) {
    geometry_checked(path, None, |starts, ends, s| {
        let (line, axis) = if horizontal { (0, 1) } else { (1, 0) };
        let SketchGeometry::Line {
            start: old_a,
            end: old_b,
        } = s.curves[line].geometry
        else {
            panic!("line")
        };
        assert!(
            (if horizontal {
                old_a.y - old_b.y
            } else {
                old_a.x - old_b.x
            })
            .abs()
                > 1.
        );
        assert!(
            (starts[line][axis] - ends[line][axis]).abs() < 1e-7,
            "H/V was NOT delivered to solver: {starts:?} {ends:?}"
        );
        assert!(
            starts.iter().zip(&s.curves).any(|(p, c)| {
                let SketchGeometry::Line { start, .. } = c.geometry else {
                    return false;
                };
                (p[0] - start.x).abs() > 0.1 || (p[1] - start.y).abs() > 0.1
            }),
            "slanted stored coordinates did not move"
        );
    });
}

struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    sketch: Sketch,
    id: ObjectId,
}
impl Fixture {
    fn slanted() -> Self {
        Self::with_points([[-20., -10.], [40., -8.], [42., 30.], [-20., 30.]])
    }
    fn with_points(points: [[f64; 2]; 4]) -> Self {
        let root = tempfile::tempdir().expect("root");
        let source = root.path().join("контур space.fcad");
        let input = root.path().join("polygon.json");
        write(
            &input,
            &json!({"request_version":1,"points_mm":points,"height_mm":10}),
        );
        reply(
            cli()
                .arg("create-sketch-extrude")
                .arg(&input)
                .arg("-o")
                .arg(&source)
                .arg("--json")
                .output()
                .expect("polygon"),
            "create-sketch-extrude",
            0,
        );
        let document = Document::open_read_only(&source).expect("document");
        let (id, sketch) = ferritecad_fixtures::sketch(&document).expect("sketch");
        document.close().expect("close");
        Self {
            root,
            source,
            sketch,
            id,
        }
    }
    fn point(&self, i: usize, at: SketchPointSelector) -> SketchPointRef {
        SketchPointRef::new(self.sketch.curves[i].id, at)
    }
    fn segment(&self, i: usize) -> SketchSegmentRef {
        SketchSegmentRef::new(
            self.point(i, SketchPointSelector::Start),
            self.point(i, SketchPointSelector::End),
        )
    }
    fn closed(&self, rules: Vec<SketchConstraintRule>) -> Vec<EditChange> {
        let joints = (0..self.sketch.curves.len()).map(|i| SketchConstraintRule::Coincident {
            a: self.point(i, SketchPointSelector::End),
            b: self.point(
                (i + 1) % self.sketch.curves.len(),
                SketchPointSelector::Start,
            ),
        });
        joints
            .chain(rules)
            .map(|rule| EditChange::AddConstraint {
                sketch: self.id,
                constraint: SketchConstraint {
                    id: StableEntityId::new(),
                    rule,
                },
            })
            .collect()
    }
    fn command(&self, source: &Path, output: &Path, changes: &[EditChange]) -> Command {
        let input = self.root.path().join("edit.json");
        write(&input, &ferritecad_fixtures::edit_request_json(changes));
        transaction::command(
            source,
            inspect(source)["content_version"]
                .as_str()
                .expect("version"),
            &input,
            output,
        )
    }
    fn apply(&self, source: &Path, name: &str, changes: &[EditChange]) -> PathBuf {
        let output = self.root.path().join(format!("{name}.fcad"));
        let before = std::fs::read(source).expect("source bytes");
        reply(
            self.command(source, &output, changes)
                .output()
                .expect("edit"),
            OP,
            0,
        );
        stored_same(source, &output);
        assert_eq!(std::fs::read(source).expect("source bytes"), before);
        output
    }
}
fn constant(value: f64) -> Expression {
    Expression::constant(value).expect("constant")
}
fn changed_constraint(change: &EditChange) -> &SketchConstraint {
    let EditChange::AddConstraint { constraint, .. } = change else {
        panic!("addition")
    };
    constraint
}

fn direction(starts: &[[f64; 2]], ends: &[[f64; 2]], i: usize) -> [f64; 2] {
    [ends[i][0] - starts[i][0], ends[i][1] - starts[i][1]]
}

fn check_rectangle(starts: &[[f64; 2]], ends: &[[f64; 2]], width: f64, height: f64) {
    for i in 0..4 {
        let d = direction(starts, ends, i);
        let expected = if i % 2 == 0 { width } else { height };
        assert!((d[0].hypot(d[1]) - expected).abs() < 1e-6);
        assert!(d[if i % 2 == 0 { 1 } else { 0 }].abs() < 1e-6);
    }
}

#[test]
fn native_dimension_pin_and_equality_replacements_publish_the_solved_geometry() {
    if !native() {
        return;
    }
    let f = Fixture::with_points([[-40., -20.], [40., -20.], [40., 20.], [-40., 20.]]);
    let distance = |i, value| SketchConstraintRule::Distance {
        a: f.point(i, SketchPointSelector::Start),
        b: f.point(i, SketchPointSelector::End),
        distance: constant(value),
    };
    let add = |rule| EditChange::AddConstraint {
        sketch: f.id,
        constraint: SketchConstraint {
            id: StableEntityId::new(),
            rule,
        },
    };
    let remove = |id| EditChange::RemoveConstraint {
        sketch: f.id,
        constraint: id,
    };
    let mut rules = (0..4)
        .map(|i| {
            let a = f.point(i, SketchPointSelector::Start);
            let b = f.point(i, SketchPointSelector::End);
            if i % 2 == 0 {
                SketchConstraintRule::Horizontal { a, b }
            } else {
                SketchConstraintRule::Vertical { a, b }
            }
        })
        .collect::<Vec<_>>();
    rules.extend([distance(0, 60.), distance(1, 30.)]);
    let changes = f.closed(rules);
    let width_id = changed_constraint(&changes[8]).id;
    let height_id = changed_constraint(&changes[9]).id;
    let rectangle = f.apply(&f.source, "length-rectangle", &changes);
    geometry_checked(&rectangle, Some(2), |starts, ends, _| {
        check_rectangle(starts, ends, 60., 30.)
    });
    let replaced = f.apply(
        &rectangle,
        "length-replaced",
        &[remove(width_id), add(distance(0, 55.))],
    );
    geometry_checked(&replaced, Some(2), |starts, ends, _| {
        check_rectangle(starts, ends, 55., 30.)
    });

    let pin = add(SketchConstraintRule::Fixed {
        point: f.point(0, SketchPointSelector::Start),
        x: constant(10.),
        y: constant(-5.),
    });
    let pinned = f.apply(&rectangle, "pinned", std::slice::from_ref(&pin));
    geometry_checked(&pinned, Some(0), |starts, ends, _| {
        check_rectangle(starts, ends, 60., 30.);
        assert!((starts[0][0] - 10.).abs() < 1e-6 && (starts[0][1] + 5.).abs() < 1e-6);
    });
    let moved = f.apply(
        &pinned,
        "pin-moved",
        &[
            remove(changed_constraint(&pin).id),
            add(SketchConstraintRule::Fixed {
                point: f.point(0, SketchPointSelector::Start),
                x: constant(-25.),
                y: constant(40.),
            }),
        ],
    );
    geometry_checked(&moved, Some(0), |starts, ends, _| {
        check_rectangle(starts, ends, 60., 30.);
        assert!((starts[0][0] + 25.).abs() < 1e-6 && (starts[0][1] - 40.).abs() < 1e-6);
    });
    let equality = add(SketchConstraintRule::EqualLength {
        a: f.segment(0),
        b: f.segment(1),
    });
    let square = f.apply(&pinned, "square", &[remove(height_id), equality.clone()]);
    let smaller = f.apply(
        &square,
        "smaller",
        &[remove(width_id), add(distance(0, 45.))],
    );
    for (path, side) in [(&square, 60.), (&smaller, 45.)] {
        geometry_checked(path, Some(0), |starts, ends, sketch| {
            check_rectangle(starts, ends, side, side);
            assert!((starts[0][0] - 10.).abs() < 1e-6 && (starts[0][1] + 5.).abs() < 1e-6);
            assert!(sketch.constraints.contains(changed_constraint(&equality)));
        });
    }

    let slanted = Fixture::slanted();
    let changes = slanted.closed(vec![SketchConstraintRule::Distance {
        a: slanted.point(0, SketchPointSelector::Start),
        b: slanted.point(0, SketchPointSelector::End),
        distance: constant(50.),
    }]);
    let output = slanted.apply(&slanted.source, "length-slanted", &changes);
    geometry_checked(&output, Some(7), |starts, ends, _| {
        let d = direction(starts, ends, 0);
        assert!((d[0].hypot(d[1]) - 50.).abs() < 1e-6);
        assert!((d[0].abs() - 50.).abs() > 0.01 && (d[1].abs() - 50.).abs() > 0.01);
    });
}

#[test]
fn native_line_orientation_add_remove_and_delivery_keep_geometry_and_identities() {
    if !native() {
        return;
    }
    let f = Fixture::slanted();
    let before = std::fs::read(&f.source).expect("bytes");
    for horizontal in [true, false] {
        let i = if horizontal { 0 } else { 1 };
        let a = f.point(i, SketchPointSelector::Start);
        let b = f.point(i, SketchPointSelector::End);
        let changes = f.closed(vec![if horizontal {
            SketchConstraintRule::Horizontal { a, b }
        } else {
            SketchConstraintRule::Vertical { a, b }
        }]);
        let output = f.root.path().join(if horizontal {
            "horizontal.fcad"
        } else {
            "vertical.fcad"
        });
        reply(
            f.command(&f.source, &output, &changes)
                .output()
                .expect("apply"),
            OP,
            0,
        );
        stored_same(&f.source, &output);
        geometry(&output, horizontal);
        let row = inspect(&output);
        assert_eq!(
            row["sketches"][0]["constraints"]
                .as_array()
                .expect("constraints")
                .len(),
            5
        );
        assert_eq!(
            row["sketches"][0]["constraints"][4]["constraint_id"],
            changed_constraint(&changes[4]).id.to_string()
        );
        let removed = f.root.path().join(format!("removed-{horizontal}.fcad"));
        reply(
            f.command(
                &output,
                &removed,
                &[EditChange::RemoveConstraint {
                    sketch: f.id,
                    constraint: changed_constraint(&changes[4]).id,
                }],
            )
            .output()
            .expect("remove"),
            OP,
            0,
        );
        stored_same(&output, &removed);
        geometry_checked(&removed, Some(8), |_, _, _| {});
        assert_eq!(
            inspect(&removed)["sketches"][0]["constraints"],
            json!(
                &row["sketches"][0]["constraints"]
                    .as_array()
                    .expect("constraints")[..4]
            )
        );
    }
    let changes = f.closed(vec![SketchConstraintRule::Horizontal {
        a: f.point(0, SketchPointSelector::Start),
        b: f.point(0, SketchPointSelector::End),
    }]);
    let output = f.root.path().join("delivery.fcad");
    let lost = f
        .command(&f.source, &output, &changes)
        .stdout(pipe::closed_pipe())
        .stderr(pipe::closed_pipe())
        .output()
        .expect("closed pipes");
    assert_eq!(lost.status.code(), Some(7));
    geometry(&output, true);
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
}

#[test]
fn native_relations_lengths_pins_and_replacement_solve_without_changing_stored_curves() {
    if !native() {
        return;
    }
    let f = Fixture::slanted();
    let before = std::fs::read(&f.source).expect("bytes");
    let changes = f.closed(vec![
        SketchConstraintRule::Parallel {
            a: f.segment(0),
            b: f.segment(2),
        },
        SketchConstraintRule::Parallel {
            a: f.segment(1),
            b: f.segment(3),
        },
        SketchConstraintRule::Perpendicular {
            a: f.segment(0),
            b: f.segment(1),
        },
        SketchConstraintRule::Distance {
            a: f.point(0, SketchPointSelector::Start),
            b: f.point(0, SketchPointSelector::End),
            distance: constant(60.0),
        },
        SketchConstraintRule::Distance {
            a: f.point(1, SketchPointSelector::Start),
            b: f.point(1, SketchPointSelector::End),
            distance: constant(30.0),
        },
    ]);
    let rectangle = f.root.path().join("relations-rect.fcad");
    reply(
        f.command(&f.source, &rectangle, &changes)
            .output()
            .expect("rectangle"),
        OP,
        0,
    );
    stored_same(&f.source, &rectangle);
    geometry_checked(&rectangle, Some(3), |starts, ends, _| {
        for i in 0..4 {
            let a = [ends[i][0] - starts[i][0], ends[i][1] - starts[i][1]];
            assert!((a[0].hypot(a[1]) - if i % 2 == 0 { 60.0 } else { 30.0 }).abs() < 1e-6);
            let j = (i + 1) % 4;
            let b = [ends[j][0] - starts[j][0], ends[j][1] - starts[j][1]];
            assert!((a[0] * b[0] + a[1] * b[1]).abs() < 1e-5);
        }
    });
    let width_id = changed_constraint(&changes[7]).id;
    let perpendicular_id = changed_constraint(&changes[6]).id;
    let smaller = f.apply(
        &rectangle,
        "relations-smaller",
        &[
            EditChange::RemoveConstraint {
                sketch: f.id,
                constraint: width_id,
            },
            EditChange::AddConstraint {
                sketch: f.id,
                constraint: SketchConstraint {
                    id: StableEntityId::new(),
                    rule: SketchConstraintRule::Distance {
                        a: f.point(0, SketchPointSelector::Start),
                        b: f.point(0, SketchPointSelector::End),
                        distance: constant(45.),
                    },
                },
            },
        ],
    );
    let freed = f.apply(
        &smaller,
        "relations-freed",
        &[EditChange::RemoveConstraint {
            sketch: f.id,
            constraint: perpendicular_id,
        }],
    );
    for (path, dof) in [(&smaller, 3), (&freed, 4)] {
        geometry_checked(path, Some(dof), |starts, ends, sketch| {
            for i in 0..4 {
                let a = direction(starts, ends, i);
                assert!((a[0].hypot(a[1]) - if i % 2 == 0 { 45. } else { 30. }).abs() < 1e-6);
                let b = direction(starts, ends, (i + 2) % 4);
                assert!((a[0] * b[1] - a[1] * b[0]).abs() < 1e-5);
                if dof == 3 {
                    let b = direction(starts, ends, (i + 1) % 4);
                    assert!((a[0] * b[0] + a[1] * b[1]).abs() < 1e-5);
                }
            }
            for change in &changes[4..6] {
                assert!(sketch.constraints.contains(changed_constraint(change)));
            }
            assert_eq!(
                sketch.constraints.iter().any(|c| c.id == perpendicular_id),
                dof == 3
            );
        });
    }
    let length = changed_constraint(changes.last().expect("length")).id;
    let equal = SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::EqualLength {
            a: f.segment(0),
            b: f.segment(1),
        },
    };
    let square = f.root.path().join("equal-square.fcad");
    reply(
        f.command(
            &rectangle,
            &square,
            &[
                EditChange::RemoveConstraint {
                    sketch: f.id,
                    constraint: length,
                },
                EditChange::AddConstraint {
                    sketch: f.id,
                    constraint: equal.clone(),
                },
            ],
        )
        .output()
        .expect("equal length"),
        OP,
        0,
    );
    stored_same(&rectangle, &square);
    geometry_checked(&square, Some(3), |starts, ends, _| {
        for i in 0..4 {
            assert!(
                ((ends[i][0] - starts[i][0]).hypot(ends[i][1] - starts[i][1]) - 60.0).abs() < 1e-6
            );
        }
    });
    let pin = SketchConstraint {
        id: StableEntityId::new(),
        rule: SketchConstraintRule::Fixed {
            point: f.point(0, SketchPointSelector::Start),
            x: constant(-30.0),
            y: constant(-20.0),
        },
    };
    let pinned = f.root.path().join("fixed-square.fcad");
    reply(
        f.command(
            &square,
            &pinned,
            &[EditChange::AddConstraint {
                sketch: f.id,
                constraint: pin.clone(),
            }],
        )
        .output()
        .expect("pin"),
        OP,
        0,
    );
    geometry_checked(&pinned, Some(1), |starts, _, _| {
        assert!((starts[0][0] + 30.0).abs() < 1e-6);
        assert!((starts[0][1] + 20.0).abs() < 1e-6);
    });
    let unpinned = f.root.path().join("unpinned.fcad");
    reply(
        f.command(
            &pinned,
            &unpinned,
            &[EditChange::RemoveConstraint {
                sketch: f.id,
                constraint: pin.id,
            }],
        )
        .output()
        .expect("unpin"),
        OP,
        0,
    );
    geometry_checked(&unpinned, Some(3), |_, _, _| {});
    // Replace a stored relation under the same UUID in request order.
    let replacement = SketchConstraint {
        id: equal.id,
        rule: SketchConstraintRule::Distance {
            a: f.point(1, SketchPointSelector::Start),
            b: f.point(1, SketchPointSelector::End),
            distance: constant(30.0),
        },
    };
    let replaced = f.root.path().join("replaced.fcad");
    reply(
        f.command(
            &unpinned,
            &replaced,
            &[
                EditChange::RemoveConstraint {
                    sketch: f.id,
                    constraint: equal.id,
                },
                EditChange::AddConstraint {
                    sketch: f.id,
                    constraint: replacement,
                },
            ],
        )
        .output()
        .expect("replace"),
        OP,
        0,
    );
    geometry_checked(&replaced, Some(3), |starts, ends, _| {
        assert!(((ends[1][0] - starts[1][0]).hypot(ends[1][1] - starts[1][1]) - 30.0).abs() < 1e-6);
    });
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
}

#[test]
fn native_conflicting_constraints_fail_the_sketch_and_skip_its_dependents_atomically() {
    if !native() {
        return;
    }
    let f = Fixture::slanted();
    let before = std::fs::read(&f.source).expect("bytes");
    let a = f.point(0, SketchPointSelector::Start);
    let b = f.point(0, SketchPointSelector::End);
    let changes = f.closed(vec![
        SketchConstraintRule::Distance {
            a,
            b,
            distance: constant(60.0),
        },
        SketchConstraintRule::Distance {
            a,
            b,
            distance: constant(30.0),
        },
    ]);
    let output = f.root.path().join("refused.fcad");
    let refusal = reply(
        f.command(&f.source, &output, &changes)
            .output()
            .expect("conflict"),
        OP,
        2,
    );
    assert!(!output.exists());
    let rebuild = &refusal["error"]["rebuild"];
    assert_eq!(rebuild["failed"][0]["id"], f.id.to_string(), "{refusal}");
    assert!(!rebuild["skipped"].as_array().expect("skipped").is_empty());
    assert!(
        refusal["error"]["constraint_conflict"].is_object(),
        "{refusal}"
    );
    assert_eq!(std::fs::read(&f.source).expect("bytes"), before);
}
