// SPDX-License-Identifier: MIT
//! §27G: the existing constraints editor on the profile of a saved Revolve
//! with a bore, driven through its real widgets and its real worker.
use super::*;
use ferritecad_document::{
    Body, DatumPlane, Dependency, DependencyRole, ObjectPayload, Point2, Revolve, RevolveAngle,
    RevolveAxis, RevolveExtent, SketchCurve, SolidOperation,
};
use ferritecad_types::Transform;

/// A stepped sector with a bore and fractional coordinates.
const SECTOR_P: [[f64; 2]; 5] = [
    [2.75, -3.25],
    [7.5, -3.25],
    [7.5, 2.125],
    [4.25, 9.5],
    [2.75, 9.5],
];
/// The same once its outer wall is dimensioned 7 mm tall.
const SECTOR_TALL: [[f64; 2]; 5] = [
    [2.75, -3.25],
    [7.5, -3.25],
    [7.5, 3.75],
    [4.25, 9.5],
    [2.75, 9.5],
];

/// The standalone Revolve frame, written without a kernel.
fn write_sector(path: &Path, points: &[[f64; 2]], degrees: f64) -> ObjectId {
    let mut d = Document::create(path).expect("document");
    let [plane, sketch, revolve, body] = std::array::from_fn(|_| ObjectId::new());
    let n = points.len();
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
                    id: StableEntityId::new(),
                    construction: false,
                    geometry: SketchGeometry::Line {
                        start: Point2::new(points[i][0], points[i][1])?,
                        end: Point2::new(points[(i + 1) % n][0], points[(i + 1) % n][1])?,
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;
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
                extent: RevolveExtent::Partial {
                    degrees: RevolveAngle::new(degrees)?,
                },
                operation: SolidOperation::NewBody,
                axis_segment: None,
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
    .expect("sector");
    d.close().expect("close");
    revolve
}
fn reading(path: &Path) -> ExtrudeEditSource {
    let d = Document::open_read_only(path).expect("open");
    let source = ExtrudeEditSource::read(&d).expect("catalog");
    d.close().expect("close");
    source
}
fn painted_prefix(out: &egui::FullOutput, prefix: &str) -> Option<String> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text().starts_with(prefix) => {
            Some(t.galley.text().to_owned())
        }
        _ => None,
    })
}
fn line(add: AddLineConstraint) -> AddSketchConstraint {
    AddSketchConstraint::Line(add)
}
fn on(curve: StableEntityId, kind: LineConstraintKind) -> AddSketchConstraint {
    line(AddLineConstraint::Line { curve, kind })
}
fn mm(value: f64) -> LineConstraintKind {
    LineConstraintKind::Distance(LineLengthMm::new(value).expect("length"))
}
/// Select a Line, then add one constraint to it through the widgets.
fn add_on(ctx: &egui::Context, e: &mut Editor, segment: usize, button: &str) {
    click(ctx, e, &format!("Segment {segment}"));
    click(ctx, e, button);
}
fn add_length(ctx: &egui::Context, e: &mut Editor, segment: usize, value: &str) {
    click(ctx, e, &format!("Segment {segment}"));
    enter_length(ctx, e, value, false);
    click(ctx, e, "Add length");
}
fn add_pin(ctx: &egui::Context, e: &mut Editor, segment: usize, x: &str, y: &str) {
    click(ctx, e, &format!("Segment {segment}"));
    click(ctx, e, "Pin Start");
    enter_field(ctx, e, "Fixed X (mm):", x, false);
    enter_field(ctx, e, "Fixed Y (mm):", y, false);
    click(ctx, e, "Add Fixed point");
}
/// The rigid dimensioning of `SECTOR_P`, through the same widgets a person
/// uses, in the order the request records it.
fn dimension_sector(ctx: &egui::Context, e: &mut Editor) {
    add_on(ctx, e, 1, "Add Horizontal");
    add_on(ctx, e, 2, "Add Vertical");
    add_on(ctx, e, 4, "Add Horizontal");
    add_on(ctx, e, 5, "Add Vertical");
    add_pin(ctx, e, 1, "2.75", "-3.25");
    add_length(ctx, e, 1, "4.75");
    add_length(ctx, e, 2, "5.375");
    add_length(ctx, e, 4, "1.5");
    add_length(ctx, e, 5, "12.75");
}
fn expected_sector(curves: &[SketchCurve]) -> Vec<AddSketchConstraint> {
    let id = |i: usize| curves[i].id;
    vec![
        on(id(0), LineConstraintKind::Horizontal),
        on(id(1), LineConstraintKind::Vertical),
        on(id(3), LineConstraintKind::Horizontal),
        on(id(4), LineConstraintKind::Vertical),
        on(
            id(0),
            LineConstraintKind::Fixed {
                at: LineEndpoint::Start,
                x: SketchCoordinateMm::new(2.75).expect("x"),
                y: SketchCoordinateMm::new(-3.25).expect("y"),
            },
        ),
        on(id(0), mm(4.75)),
        on(id(1), mm(5.375)),
        on(id(3), mm(1.5)),
        on(id(4), mm(12.75)),
    ]
}

/// Without a kernel or a solver: the window names the Revolve and its kept
/// angle, the widgets build the rigid request one checkpoint at a time, a
/// refused addition keeps the draft, and Replace length on a stored length is
/// one exact removal plus one addition.
#[test]
fn revolve_constraint_widgets_name_the_revolve_build_and_replace_a_length() {
    let root = tempfile::tempdir().expect("dir");
    let path = root.path().join("sector.fcad");
    let revolve = write_sector(&path, &SECTOR_P, 137.5);
    let source = reading(&path);
    let choice = &source.constraint_sketches[0];
    assert_eq!(choice.refusal, None);
    assert_eq!(choice.height_mm, None, "a Revolve has no height");
    let Some(SketchProfileUse::PartialRevolve {
        feature,
        axis_segment: None,
        degrees,
        ..
    }) = choice.profile_use
    else {
        panic!("a sector with a bore: {:?}", choice.profile_use)
    };
    assert_eq!((feature, degrees.degrees()), (revolve, 137.5));
    let curves = choice.stored.as_ref().expect("stored").curves.clone();
    let mut e = Editor::default();
    assert!(e.begin(&path, &source, choice.sketch));
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    let owner = painted_prefix(&frame(&ctx, &mut e, vec![]), "Profile of Revolve")
        .expect("the owning Revolve is named");
    assert_eq!(
        owner,
        format!(
            "Profile of Revolve {revolve}: 137.5° about the sketch Y axis. The turn and the axis \
             are kept; the solved profile must keep its bore clear of the axis."
        )
    );
    dimension_sector(&ctx, &mut e);
    let rigid = expected_sector(&curves);
    assert_eq!(history_state(&e).0.add, rigid);
    assert_eq!(
        history_state(&e).1.len(),
        rigid.len(),
        "one checkpoint each"
    );
    click(&ctx, &mut e, "Undo");
    assert_eq!(history_state(&e).0.add, rigid[..rigid.len() - 1]);
    click(&ctx, &mut e, "Redo");
    assert_eq!(history_state(&e).0.add, rigid);
    // A second H on the bottom Line is refused and keeps everything.
    let kept = history_state(&e);
    add_on(&ctx, &mut e, 1, "Add Horizontal");
    assert!(e.draft.as_ref().expect("draft").refusal.is_some());
    assert_eq!(history_state(&e), kept);
    click(&ctx, &mut e, "Save constraints copy…");
    let request = e.take_request().expect("widget request");
    assert_eq!(request.edits.add, rigid);
    assert!(request.edits.remove.is_empty());
    assert_eq!(
        (request.source.as_path(), request.sketch, request.expected),
        (path.as_path(), choice.sketch, source.version)
    );
    assert_eq!(
        history_state(&e),
        kept,
        "Save keeps the draft until a reply"
    );
    // Where there is Open CASCADE but no solver, the real worker refuses
    // with the typed reason; the draft, its history and the source stay.
    if ferritecad_occt::is_available() && !ferritecad_sketch_solver::is_available() {
        let before = std::fs::read(&path).expect("source");
        let mut r = request.clone();
        r.destination = root.path().join("copy.fcad");
        let mut state = crate::edits::Edits::default();
        let (tx, rx) = std::sync::mpsc::channel();
        state
            .start_constraints(r, move |r, g, c| {
                crate::edits::spawn_constraint_edit(r, c, move |result| {
                    tx.send((g, result)).expect("reply")
                })
            })
            .expect("worker");
        let (g, result) = rx.recv().expect("worker reply");
        let error = result.as_ref().expect_err("no solver").to_string();
        assert!(error.contains("planegcs"), "{error}");
        assert_eq!(finish_edit(&mut e, &mut state, g, result), None);
        assert_eq!(history_state(&e), kept, "the draft survives the refusal");
        assert!(!root.path().join("copy.fcad").exists());
        assert_eq!(std::fs::read(&path).expect("source"), before);
    }

    // A stored length, written without a solver, is replaced exactly.
    let mut document = Document::open(&path).expect("open");
    let prepared = ferritecad_document::prepare_sketch_constraints(
        &document,
        choice.sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: rigid.clone(),
        },
    )
    .expect("prepared");
    document
        .write_sketch_constraints(&prepared)
        .expect("stored constraints");
    document.close().expect("close");
    let stored = reading(&path);
    let wall = stored.constraint_sketches[0]
        .stored
        .as_ref()
        .expect("stored")
        .constraints
        .iter()
        .find(|c| matches!(c.rule, SketchConstraintRule::Distance { distance, .. } if distance == 5.375))
        .expect("the wall height")
        .id;
    let mut e = Editor::default();
    assert!(e.begin(&path, &stored, choice.sketch));
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    click(&ctx, &mut e, "Segment 2");
    enter_length(&ctx, &mut e, "7", false);
    click(&ctx, &mut e, "Replace length");
    assert_eq!(
        history_state(&e).0,
        SketchConstraintEdits {
            remove: vec![wall],
            add: vec![on(curves[1].id, mm(7.))],
        }
    );
    assert_eq!(history_state(&e).1.len(), 1, "one checkpoint");
}

/// With Open CASCADE and PlaneGCS: the widgets' rigid request is published
/// by the real worker — after a refusal for an occupied destination that
/// keeps the draft — and by the peer CLI from the same source; both copies
/// hold the same model with only the new constraint UUIDs different, the
/// same solved sector and byte-identical STL and FBX. Then a published copy
/// is reopened and its wall height replaced the same two ways.
#[test]
fn native_revolve_constraint_worker_and_cli_publish_the_same_solved_sector() {
    use crate::creates::tests::ferritecad;
    if !ferritecad_occt::is_available() || !ferritecad_sketch_solver::is_available() {
        assert_ne!(
            std::env::var("FERRITECAD_REQUIRE_PLANEGCS").as_deref(),
            Ok("1")
        );
        eprintln!("skipped: the Revolve constraint worker requires OCCT and PlaneGCS");
        return;
    }
    let root = tempfile::tempdir().expect("dir");
    let source = root.path().join("sector.fcad");
    let input = root.path().join("create.json");
    std::fs::write(
        &input,
        r#"{"request_version":2,"points_mm":[[2.75,-3.25],[7.5,-3.25],[7.5,2.125],[4.25,9.5],[2.75,9.5]],"axis":"sketch_y","extent":{"kind":"angle","degrees":137.5}}"#,
    )
    .expect("request");
    let o = std::process::Command::new(ferritecad())
        .arg("create-sketch-revolve")
        .arg(&input)
        .arg("-o")
        .arg(&source)
        .arg("--json")
        .output()
        .expect("create");
    assert!(o.status.success(), "{o:?}");
    let original = std::fs::read(&source).expect("source");
    let open = |path: &Path| {
        let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
        ferritecad_scene::snapshot_of(
            path,
            &mut k,
            |k, b| k.import_step(b),
            &Default::default(),
            &OperationContext::default(),
        )
        .expect("async Open route")
        .edit_source
        .expect("catalog")
    };
    let first = open(&source);
    let choice = first.constraint_sketches[0].clone();
    let id = choice.sketch;
    let curves = choice.stored.as_ref().expect("stored").curves.clone();
    let ctx = egui::Context::default();
    let mut e = Editor::default();
    assert!(e.begin(&source, &first, id));
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    dimension_sector(&ctx, &mut e);
    click(&ctx, &mut e, "Save constraints copy…");
    let request = e.take_request().expect("widget request");
    assert_eq!(request.edits.add, expected_sector(&curves));
    let ui = publish_both(
        &mut e,
        request,
        root.path(),
        "ui",
        &source,
        &first,
        id,
        ferritecad,
    );
    let (dof, starts) = solved(&ui.0);
    assert_eq!(dof, 0);
    for (s, e) in starts.iter().zip(SECTOR_P) {
        assert!(
            (s[0] - e[0]).abs() < 1e-7 && (s[1] - e[1]).abs() < 1e-7,
            "{starts:?}"
        );
    }
    assert_eq!(std::fs::read(&source).expect("source"), original);

    // Reopened from the UI copy, as async Open shows it after publication,
    // the wall height is replaced by exact UUID through Replace length.
    let second = open(&ui.0);
    let wall = second.constraint_sketches[0]
        .stored
        .as_ref()
        .expect("stored")
        .constraints
        .iter()
        .find(|c| matches!(c.rule, SketchConstraintRule::Distance { distance, .. } if distance == 5.375))
        .expect("the wall height")
        .id;
    assert!(e.begin(&ui.0, &second, id));
    for _ in 0..3 {
        frame(&ctx, &mut e, vec![]);
    }
    click(&ctx, &mut e, "Segment 2");
    enter_length(&ctx, &mut e, "7", false);
    click(&ctx, &mut e, "Replace length");
    click(&ctx, &mut e, "Save constraints copy…");
    let request = e.take_request().expect("replacement");
    assert_eq!(request.edits.remove, vec![wall]);
    assert_eq!(request.edits.add, vec![on(curves[1].id, mm(7.))]);
    let tall = publish_both(
        &mut e,
        request,
        root.path(),
        "tall",
        &ui.0,
        &second,
        id,
        ferritecad,
    );
    let (dof, starts) = solved(&tall.0);
    assert_eq!(dof, 0);
    for (s, e) in starts.iter().zip(SECTOR_TALL) {
        assert!(
            (s[0] - e[0]).abs() < 1e-7 && (s[1] - e[1]).abs() < 1e-7,
            "{starts:?}"
        );
    }
    assert_ne!(
        std::fs::read(tall.0.with_extension("stl")).expect("tall"),
        std::fs::read(ui.0.with_extension("stl")).expect("rigid"),
        "a taller wall is a different Body"
    );
}

/// The real worker publishes `request` — first refused by an occupied
/// destination, which keeps the draft and its history — and the peer CLI
/// publishes the same additions from the same source. Returns both copies,
/// compared cell by cell with only the new constraint UUIDs differing, and
/// with byte-identical STL and FBX.
#[allow(clippy::too_many_arguments)]
fn publish_both(
    e: &mut Editor,
    request: EditSketchConstraintsRequest,
    root: &Path,
    name: &str,
    source: &Path,
    reading: &ExtrudeEditSource,
    id: ObjectId,
    ferritecad: fn() -> PathBuf,
) -> (PathBuf, PathBuf) {
    let history = history_state(e);
    let kept = request.edits.clone();
    let mut state = crate::edits::Edits::default();
    let ui = root.join(format!("{name}-ui.fcad"));
    for occupied in [true, false] {
        let mut r = request.clone();
        r.destination = if occupied {
            root.join(format!("{name}-busy.fcad"))
        } else {
            ui.clone()
        };
        if occupied {
            std::fs::write(&r.destination, b"keep").expect("busy");
        }
        let (tx, rx) = std::sync::mpsc::channel();
        state
            .start_constraints(r, move |r, g, c| {
                crate::edits::spawn_constraint_edit(r, c, move |result| {
                    tx.send((g, result)).expect("reply")
                })
            })
            .expect("worker");
        let (g, result) = rx.recv().expect("worker reply");
        let open = finish_edit(e, &mut state, g, result);
        if occupied {
            assert!(open.is_none());
            assert_eq!(e.draft.as_ref().expect("retained").edits, kept);
            assert_eq!(history_state(e), history);
            assert_eq!(
                std::fs::read(root.join(format!("{name}-busy.fcad"))).expect("busy"),
                b"keep"
            );
        } else {
            assert_eq!(open, Some(ui.clone()));
            assert!(!e.active());
        }
    }
    let removals = kept
        .remove
        .iter()
        .map(|id| format!(r#""{id}""#))
        .collect::<Vec<_>>()
        .join(",");
    let additions = kept
        .add
        .iter()
        .map(peer_addition)
        .collect::<Vec<_>>()
        .join(",");
    let input = root.join(format!("{name}.json"));
    std::fs::write(
        &input,
        format!(r#"{{"request_version":1,"remove":[{removals}],"add":[{additions}]}}"#),
    )
    .expect("typed IDs to peer request");
    let cli = root.join(format!("{name}-cli.fcad"));
    let o = std::process::Command::new(ferritecad())
        .arg("edit-sketch-constraints-copy")
        .arg(source)
        .arg("--sketch")
        .arg(id.to_string())
        .arg("--expect-version")
        .arg(reading.version.content.to_string())
        .arg("--request")
        .arg(&input)
        .arg("-o")
        .arg(&cli)
        .arg("--json")
        .output()
        .expect("peer CLI");
    assert!(o.status.success(), "{o:?}");
    let a = crate::sketch::drag_tests::sql_facts(&ui);
    let b = crate::sketch::drag_tests::sql_facts(&cli);
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    for (table, rows) in &a {
        if table != "objects" {
            assert_eq!(rows, &b[table], "{table}: every cell, rowids included");
        }
    }
    let (x, y) = (
        Document::open_read_only(&ui).expect("UI"),
        Document::open_read_only(&cli).expect("CLI"),
    );
    assert_eq!(x.meta(), y.meta());
    assert_eq!(
        x.topology_refs().expect("refs"),
        y.topology_refs().expect("refs")
    );
    for old in x.objects().expect("objects") {
        let new = y.object(old.id).expect("read").expect("same id");
        match (&old.payload, &new.payload) {
            (ObjectPayload::Sketch(s), ObjectPayload::Sketch(t)) => {
                assert_eq!((&s.plane, &s.curves), (&t.plane, &t.curves));
                assert_eq!(s.constraints.len(), t.constraints.len());
                let known: Vec<_> = {
                    let d = Document::open_read_only(source).expect("source");
                    let ids = match d.object(id).expect("read").expect("sketch").payload {
                        ObjectPayload::Sketch(s) => s.constraints.iter().map(|c| c.id).collect(),
                        _ => vec![],
                    };
                    d.close().expect("close");
                    ids
                };
                for (p, q) in s.constraints.iter().zip(&t.constraints) {
                    assert_eq!(p.rule, q.rule);
                    // Kept constraints keep their UUIDs; only new ones differ.
                    assert_eq!(p.id == q.id, known.contains(&p.id), "{p:?} / {q:?}");
                }
            }
            _ => assert_eq!(old, new),
        }
    }
    x.close().expect("close");
    y.close().expect("close");
    let mut outputs = vec![];
    for path in [&ui, &cli] {
        let mut files = vec![];
        for (op, suffix) in [("export-stl", "stl"), ("export-fbx", "fbx")] {
            let out = path.with_extension(suffix);
            let o = std::process::Command::new(ferritecad())
                .arg(op)
                .arg(path)
                .arg("-o")
                .arg(&out)
                .arg("--json")
                .output()
                .expect("export");
            assert!(o.status.success(), "{o:?}");
            files.push(std::fs::read(&out).expect("artifact"));
        }
        outputs.push(files);
        if let Some(dir) = std::env::var_os("FCAD_REVOLVE_CONSTRAINT_ARTIFACTS") {
            std::fs::create_dir_all(&dir).expect("dir");
            std::fs::copy(path, Path::new(&dir).join(path.file_name().expect("name")))
                .expect("artifact");
        }
    }
    assert_eq!(
        outputs[0], outputs[1],
        "UI and CLI: the same STL and FBX bytes"
    );
    (ui, cli)
}

/// The solved sketch of a published copy, from a cold rebuild.
fn solved(path: &Path) -> (usize, Vec<[f64; 2]>) {
    let d = Document::open_read_only(path).expect("copy");
    let mut k = ferritecad_occt::OcctKernel::new().expect("kernel");
    let b = ferritecad_eval::rebuild_cold(&d, &mut k, &OperationContext::default())
        .expect("cold rebuild");
    let sketch = d
        .objects()
        .expect("objects")
        .iter()
        .find_map(|o| matches!(o.payload, ObjectPayload::Sketch(_)).then_some(o.id))
        .expect("sketch");
    let dof = b.solve_report(sketch).expect("report").degrees_of_freedom();
    let starts = b
        .sketch_presentation(sketch)
        .expect("solved")
        .curves()
        .iter()
        .map(|c| match *c.geometry() {
            SketchGeometry::Line { start, .. } => [start.x, start.y],
            _ => panic!("a Line"),
        })
        .collect();
    for r in d.topology_refs().expect("refs") {
        assert_eq!(b.resolve(&r).expect("resolves").len(), 1);
    }
    b.release_all(&mut k);
    d.close().expect("close");
    (dof, starts)
}
