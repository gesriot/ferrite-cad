// SPDX-License-Identifier: MIT
use super::*;
use crate::cut_facts::CutObservation;
use ferritecad_document::{EditChange, ExpressionSlot};
use ferritecad_jobs::DocumentSession;
use ferritecad_kernel::OperationContext;

struct Pair {
    root: tempfile::TempDir,
    original: PathBuf,
    original_bytes: Vec<u8>,
    path: PathBuf,
    session: DocumentSession,
    kernel: ferritecad_occt::OcctKernel,
    count: usize,
}
impl Pair {
    fn new(tools: &[CutSpec]) -> Self {
        let root = tempfile::tempdir().expect("root");
        let path = history::fixture(root.path(), tools);
        let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
        let session =
            DocumentSession::open_in(root.path(), &path, Default::default()).expect("session");
        session
            .current()
            .load_scene(
                &mut kernel,
                |k, b| k.import_step(b),
                &Default::default(),
                &OperationContext::default(),
            )
            .expect("baseline scene");
        Self {
            original: path.clone(),
            original_bytes: std::fs::read(&path).expect("original"),
            path,
            root,
            session,
            kernel,
            count: 0,
        }
    }
    fn wall(&self, sketch: ObjectId, left: f64) -> Vec<EditChange> {
        let d = Document::open_read_only(&self.path).expect("document");
        let ObjectPayload::Sketch(s) = d.object(sketch).expect("object").expect("sketch").payload
        else {
            panic!("sketch")
        };
        let moves: Vec<_> = s
            .curves
            .iter()
            .filter_map(|c| {
                let SketchGeometry::Line { start, .. } = c.geometry else {
                    panic!("line")
                };
                (start.x != 80.).then_some(SketchVertex {
                    curve_id: c.id,
                    start_mm: [left, start.y],
                })
            })
            .collect();
        ferritecad_document::move_sketch_vertices(sketch, &s, &moves).expect("moves")
    }
    fn apply(&mut self, changes: Vec<EditChange>, refused: &[String]) {
        self.count += 1;
        let destination = self.root.path().join(format!("edit-{}.fcad", self.count));
        let request = self.root.path().join("request.json");
        write(&request, &ferritecad_fixtures::edit_request_json(&changes));
        let version = inspect(&self.path)["content_version"]
            .as_str()
            .expect("version")
            .to_owned();
        let before = std::fs::read(&self.path).expect("source bytes");
        let session_before = self.session.current().version();
        let wire = reply(
            super::super::transaction::command(&self.path, &version, &request, &destination)
                .output()
                .expect("CLI"),
            "edit",
            if refused.is_empty() { 0 } else { 2 },
        );
        let context = OperationContext::default();
        let step = self.session.begin_step().edit(
            changes,
            self.session.current().version(),
            &mut self.kernel,
            |k, b| k.import_step(b),
            &context,
        );
        if refused.is_empty() {
            self.session
                .commit_step(step.expect("session accepts"))
                .expect("commit");
            self.path = destination;
            let a = cold(&self.path, &mut self.kernel);
            let b = cold(self.session.current().path(), &mut self.kernel);
            assert!((a - b).abs() < 1e-7, "copy/session volume: {a} vs {b}");
        } else {
            let error = step.expect_err("session refusal");
            assert!(
                ferritecad_eval::RebuildRefusal::from_error(&error).is_some(),
                "{error}"
            );
            for message in refused {
                assert!(wire.to_string().contains(message), "{message}: {wire}");
                assert!(error.to_string().contains(message), "{message}: {error}");
            }
            assert!(!destination.exists());
            assert_eq!(self.session.current().version(), session_before);
            assert_eq!(std::fs::read(&self.path).expect("source bytes"), before);
        }
        assert_eq!(
            std::fs::read(&self.original).expect("original bytes"),
            self.original_bytes
        );
    }
    fn continue_cut(&mut self, saved: &CutObservation) {
        let dest = self
            .root
            .path()
            .join(format!("continued-{}.fcad", self.count));
        edits::edit(
            &self.path,
            &dest,
            saved,
            CutSpec {
                center_mm: saved.center_mm,
                radius_mm: 0.75,
                extent: saved.extent,
            },
            0,
        );
        cold(&dest, &mut self.kernel);
        let d = Document::open_read_only(&dest).expect("reopen");
        let body = tip(&d).0;
        let changes = CutSpec {
            center_mm: [60., 40.],
            radius_mm: 1.,
            extent: CutDepth::Blind { depth_mm: 3. },
        }
        .additions(&d, body);
        let request = self.root.path().join("add-after-edit.json");
        write(&request, &ferritecad_fixtures::edit_request_json(&changes));
        let added = self
            .root
            .path()
            .join(format!("continued-add-{}.fcad", self.count));
        reply(
            crate::transaction::command(
                &dest,
                &d.content_version().expect("version").to_string(),
                &request,
                &added,
            )
            .output()
            .expect("CLI"),
            "edit",
            0,
        );
        let old_volume = cold(&dest, &mut self.kernel);
        let new_volume = cold(&added, &mut self.kernel);
        assert!((old_volume - new_volume - std::f64::consts::PI * 3.).abs() < 1e-6);
    }
}
fn cold(path: &Path, kernel: &mut ferritecad_occt::OcctKernel) -> f64 {
    let d = Document::open_read_only(path).expect("reopen");
    let built = ferritecad_eval::rebuild_cold(&d, kernel, &OperationContext::default())
        .expect("cold rebuild");
    assert!(built.all_built(), "{:?}", built.report());
    for r in d.topology_refs().expect("names") {
        assert!(
            !built.resolve(&r).expect("name resolves").is_empty(),
            "{r:?}"
        );
    }
    let (body, _) = tip(&d);
    let volume = kernel
        .shape_stats(built.shape(body).expect("body"))
        .expect("stats")
        .1;
    built.release_all(kernel);
    volume
}
fn circle(saved: &CutObservation, center: [f64; 2], radius: f64) -> EditChange {
    EditChange::SetCurveGeometry {
        sketch: saved.tool_sketch,
        curve: saved.tool_curve,
        geometry: SketchGeometry::Circle {
            center: ferritecad_document::Point2 {
                x: center[0],
                y: center[1],
            },
            radius,
        },
    }
}

pub(super) fn run() {
    let tool = CutSpec {
        center_mm: [4., 7.],
        radius_mm: 1.5,
        extent: CutDepth::Blind { depth_mm: 12. },
    };
    let mut p = Pair::new(&[tool]);
    let saved = history::ordered(&p.path).remove(0);
    let refusal = vec![saved.feature.to_string(), "touches side or vertex".into()];
    p.apply(p.wall(saved.profile_sketch, 2.5), &refusal);
    p.apply(vec![circle(&saved, [1.5, 7.], 1.5)], &refusal);
    // Both sides of the guard are accepted, without nudging coordinates.
    for left in [2.25, 2.75] {
        p.apply(p.wall(saved.profile_sketch, left), &[]);
        let d = 4. - left;
        let area = if d >= 1.5 {
            std::f64::consts::PI * 1.5 * 1.5
        } else {
            1.5 * 1.5 * (-d / 1.5).acos() + d * (1.5 * 1.5 - d * d).sqrt()
        };
        let expected = ((80. - left) * 50. - area) * 12.;
        let actual = cold(&p.path, &mut p.kernel);
        assert!((actual - expected).abs() < 1e-6, "{actual} vs {expected}");
    }
    p.continue_cut(&saved);
    // Undo and Redo preserve the original slot snapshots.
    let outside = p.session.current().version();
    let undo = p.session.begin_undo().expect("undo");
    p.session.commit_move(undo).expect("undo commit");
    assert!(
        crate::cut_facts::observed(p.session.current().path())
            .iter()
            .any(|c| c.feature == saved.feature)
    );
    let redo = p.session.begin_redo().expect("redo");
    p.session.commit_move(redo).expect("redo commit");
    assert_eq!(p.session.current().version(), outside);
    p.apply(p.wall(saved.profile_sketch, 0.), &[]);
    p.continue_cut(&saved);
    p.apply(p.wall(saved.profile_sketch, 2.75), &[]);
    p.apply(vec![circle(&saved, [4., 7.], 1.)], &[]);
    p.continue_cut(&saved);
    // An open slot must acquire its floor name when the base grows.
    p.apply(vec![circle(&saved, [4., 7.], 1.5)], &[]);
    p.continue_cut(&saved);
    p.apply(
        vec![
            EditChange::constant(saved.base_feature, ExpressionSlot::ExtrudeDistance, 15.)
                .expect("height"),
        ],
        &[],
    );
    p.apply(p.wall(saved.profile_sketch, 0.), &[]);
    for path in [&p.path, p.session.current().path()] {
        let d = Document::open_read_only(path).expect("reopen");
        assert!(
            d.topology_refs()
                .expect("names")
                .iter()
                .any(|r| r.owner == saved.feature
                    && r.producer_feature == saved.feature
                    && r.output_role
                        == SemanticRole::ExtrudeCap {
                            side: ferritecad_document::CapSide::End
                        })
        );
    }
    p.continue_cut(&saved);
    let mut p = Pair::new(&[
        tool,
        CutSpec {
            center_mm: [12., 7.],
            ..tool
        },
    ]);
    let cuts = history::ordered(&p.path);
    p.apply(
        vec![circle(&cuts[1], [7., 7.], 1.5)],
        &[
            cuts[0].feature.to_string(),
            cuts[1].feature.to_string(),
            "touches Cut".into(),
        ],
    );
    p.apply(vec![circle(&cuts[1], [6.75, 7.], 1.5)], &[]);
    p.continue_cut(&cuts[0]);
}

#[test]
fn native_contacts_use_the_solved_tool_and_solved_base_lines() {
    if !ferritecad_fixtures::native_dependencies(&[
        (ferritecad_occt::is_available(), "OCCT"),
        (ferritecad_sketch_solver::is_available(), "PLANEGCS"),
    ]) {
        return;
    }
    use ferritecad_document::*;
    let tool = CutSpec {
        center_mm: [4., 7.],
        radius_mm: 1.5,
        extent: CutDepth::Blind { depth_mm: 12. },
    };
    let mut pair = Pair::new(&[tool]);
    let saved = history::ordered(&pair.path).remove(0);
    let constraint = StableEntityId::new();
    let radius = |value| EditChange::AddConstraint {
        sketch: saved.tool_sketch,
        constraint: SketchConstraint {
            id: constraint,
            rule: SketchConstraintRule::Radius {
                curve: saved.tool_curve,
                radius: Expression::constant(value).expect("radius"),
            },
        },
    };
    pair.apply(
        vec![radius(4.)],
        &[saved.feature.to_string(), "touches side or vertex".into()],
    );
    pair.apply(vec![radius(3.75)], &[]);
    pair.continue_cut(&saved);
    pair.apply(
        vec![EditChange::RemoveConstraint {
            sketch: saved.tool_sketch,
            constraint,
        }],
        &[],
    );
    pair.continue_cut(&saved);
    let d = Document::open_read_only(&pair.path).expect("document");
    let ObjectPayload::Sketch(sketch) = d
        .object(saved.profile_sketch)
        .expect("row")
        .expect("sketch")
        .payload
    else {
        panic!("sketch")
    };
    let mut add = Vec::new();
    for c in &sketch.curves {
        let SketchGeometry::Line { start, end } = c.geometry else {
            panic!("line")
        };
        add.push(AddSketchConstraint::Line(AddLineConstraint::Line {
            curve: c.id,
            kind: if start.y == end.y {
                LineConstraintKind::Horizontal
            } else {
                LineConstraintKind::Vertical
            },
        }));
    }
    add.push(AddSketchConstraint::Line(AddLineConstraint::Line {
        curve: sketch.curves[0].id,
        kind: LineConstraintKind::Fixed {
            at: LineEndpoint::Start,
            x: SketchCoordinateMm::new(2.5).expect("x"),
            y: SketchCoordinateMm::new(0.).expect("y"),
        },
    }));
    for (index, length) in [(0, 77.5), (1, 50.)] {
        add.push(AddSketchConstraint::Line(AddLineConstraint::Line {
            curve: sketch.curves[index].id,
            kind: LineConstraintKind::Distance(LineLengthMm::new(length).expect("length")),
        }));
    }
    let changes = ferritecad_fixtures::constraint_changes(
        &d,
        saved.profile_sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add,
        },
    )
    .expect("base constraints");
    d.close().expect("close");
    pair.apply(
        changes,
        &[saved.feature.to_string(), "touches side or vertex".into()],
    );
}
