// SPDX-License-Identifier: MIT
use super::*;
use crate::{
    AnnularExtrusion, CircleExtrusion, CreateDocumentRequest, FullTurnRevolution, NewDocument,
    RevolveAngle, create_document_with_kernel,
};
use ferritecad_document::{
    AddLineConstraint, AddSketchConstraint, Document, EndCondition, LineConstraintKind,
    ObjectPayload, Point2, SketchConstraintEdits, SketchGeometry, SketchVertex,
    move_sketch_vertices, sketch_constraint_changes,
};
use ferritecad_fixtures::feature_changes::{base_profile, body, circular_tool, edge_addition};
use std::path::Path;

fn fixture(path: &Path, operation: &str) -> Result<Vec<EditChange>> {
    let content = match operation {
        "circle" => Some(NewDocument::CircleExtrude(CircleExtrusion::new(
            [10., 10.],
            5.,
            10.,
        )?)),
        "annulus" => Some(NewDocument::AnnularExtrude(AnnularExtrusion::new(
            [10., 10.],
            5.,
            2.,
            10.,
        )?)),
        "revolve_angle" => Some(NewDocument::SketchPartialRevolve {
            profile: FullTurnRevolution::new(vec![[5., 0.], [15., 0.], [15., 20.], [5., 20.]])?,
            angle: RevolveAngle::new(std::f64::consts::PI)?,
        }),
        _ => None,
    };
    if let Some(content) = content {
        create_document_with_kernel(
            CreateDocumentRequest::new(path, content, "keep"),
            || Ok(TopologyMock::new()),
            &OperationContext::default(),
        )?;
    } else {
        ferritecad_fixtures::write_plate(path)?;
    }
    let mut doc = Document::open(path)?;
    let body = body(&doc)?;
    if operation == "revolve_angle" {
        let id = doc
            .objects()?
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Revolve(_)))
            .expect("partial turn")
            .id;
        return Ok(vec![EditChange::constant(
            id,
            ExpressionSlot::RevolveAngle,
            200f64.to_radians(),
        )?]);
    }
    let (base, sketch, profile) = base_profile(&doc, body)?;
    let changes = match operation {
        "height" => vec![EditChange::constant(
            base,
            ExpressionSlot::ExtrudeDistance,
            12.,
        )?],
        "vertices" => {
            let vertices = profile
                .curves
                .iter()
                .map(|c| {
                    let SketchGeometry::Line { start, .. } = c.geometry else {
                        unreachable!("plate line")
                    };
                    SketchVertex {
                        curve_id: c.id,
                        start_mm: [start.x * 0.9, start.y],
                    }
                })
                .collect::<Vec<_>>();
            move_sketch_vertices(sketch, &profile, &vertices)?
        }
        "constraints" => sketch_constraint_changes(
            sketch,
            &profile,
            &SketchConstraintEdits {
                remove: vec![],
                add: vec![AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve: profile.curves[0].id,
                    kind: LineConstraintKind::Horizontal,
                })],
            },
        )?,
        "circle" | "annulus" => vec![EditChange::SetCurveGeometry {
            sketch,
            curve: profile.curves[0].id,
            geometry: SketchGeometry::Circle {
                center: Point2::new(10., 10.)?,
                radius: 6.,
            },
        }],
        "add_cut" | "cut_parameters" => {
            let tool = circular_tool(&doc, body, [20., 20.], 2., EndCondition::ThroughAll)?;
            if operation == "add_cut" {
                tool.changes
            } else {
                doc.write_changes(&tool.changes)?;
                vec![EditChange::SetCurveGeometry {
                    sketch: tool.sketch,
                    curve: tool.curve,
                    geometry: SketchGeometry::Circle {
                        center: Point2::new(20., 20.)?,
                        radius: 2.5,
                    },
                }]
            }
        }
        "add_fillet" | "add_chamfer" | "fillet_radius" | "chamfer_distance" => {
            let chamfer = operation.contains("chamfer");
            let addition = edge_addition(&doc, body, 0, 2., chamfer)?;
            if operation.starts_with("add_") {
                vec![addition]
            } else {
                let id = addition.object();
                doc.write_changes(&[addition])?;
                let slot = if chamfer {
                    ExpressionSlot::ChamferDistance
                } else {
                    ExpressionSlot::FilletRadius
                };
                vec![EditChange::constant(id, slot, 2.5)?]
            }
        }
        _ => unreachable!("listed operation"),
    };
    doc.close()?;
    Ok(changes)
}

// Extrude, Revolve, Boolean, Fillet, Chamfer, tessellations, sketch solves.
type Work = [usize; 7];
fn check_work(c: Calls, before: u64, expected: Work) {
    let solves = (ferritecad_sketch_solver::native_solves() - before) as usize;
    assert_eq!(
        [
            c.extrude_count,
            c.revolve_count,
            c.boolean,
            c.fillet,
            c.chamfer,
            c.tessellate,
            solves
        ],
        expected
    );
    assert_eq!(c.restore, 0);
    if expected[..6] == [0; 6] {
        assert_eq!(c, Calls::default());
    }
}

fn exercise<K: Oracle>() -> Result<()> {
    for (operation, work, old, union, new) in [
        ("height", [1, 0, 0, 0, 0, 1, 0], 1, 2, 1),
        ("vertices", [1, 0, 0, 0, 0, 1, 0], 1, 2, 1),
        ("constraints", [0, 0, 0, 0, 0, 0, 1], 1, 1, 1),
        ("circle", [1, 0, 0, 0, 0, 1, 0], 1, 2, 1),
        ("annulus", [1, 0, 0, 0, 0, 1, 0], 1, 2, 1),
        ("cut_parameters", [1, 0, 1, 0, 0, 1, 0], 2, 3, 2),
        ("add_cut", [1, 0, 1, 0, 0, 1, 0], 1, 2, 2),
        ("add_fillet", [0, 0, 0, 1, 0, 1, 0], 1, 2, 2),
        ("fillet_radius", [0, 0, 0, 1, 0, 1, 0], 2, 3, 2),
        ("add_chamfer", [0, 0, 0, 0, 1, 1, 0], 1, 2, 2),
        ("chamfer_distance", [0, 0, 0, 0, 1, 1, 0], 2, 3, 2),
        ("revolve_angle", [0, 1, 0, 0, 0, 1, 0], 1, 2, 1),
    ] {
        if operation == "constraints"
            && !ferritecad_fixtures::native_dependencies(&[(
                ferritecad_sketch_solver::is_available(),
                "PLANEGCS",
            )])
        {
            continue;
        }
        let root = tempfile::tempdir()?;
        let path = root.path().join("source.fcad");
        let change = fixture(&path, operation)?;
        let source_bytes = std::fs::read(&path)?;
        let ctx = OperationContext::default();
        let mut live = LiveDocument::open(
            DocumentSession::open(&path)?,
            Counting::wrap(K::fresh()?),
            |_: &mut Counting<K>, _: &[u8]| unreachable!("no imports"),
            TessellationParams::default(),
            &ctx,
        )?;
        let original = live.session().current();
        let bytes = std::fs::read(original.path())?;
        let scene = live.scene().clone();
        assert_eq!(live.kernel.inner.population(), old);
        live.kernel.calls.take();
        let solves = ferritecad_sketch_solver::native_solves();
        let mut step = live.apply(original.version(), change, &ctx)?;
        check_work(step.owner.kernel.calls.take(), solves, work);
        assert_eq!(step.owner.kernel.inner.population(), union, "{operation}");
        assert_eq!(step.owner.session.current().version(), original.version());
        assert_eq!(step.owner.session.current().path(), original.path());
        assert!(!step.owner.session.can_undo());
        assert_eq!(std::fs::read(original.path())?, bytes);
        assert_eq!(step.owner.picture.scene, scene);
        compare(&mut step)?;
        let next = step.snapshot().version();
        let private = step.snapshot().path().to_owned();
        assert_ne!(next, original.version());
        step.commit_step()?;
        assert_eq!(live.session().current().version(), next);
        assert_eq!(live.session().current().path(), private);
        assert_eq!(live.kernel.inner.population(), new);
        live.kernel.calls.take();
        let solves = ferritecad_sketch_solver::native_solves();
        let mut undo = live.undo(&ctx)?;
        let undo_work = if operation.starts_with("add_") {
            [0, 0, 0, 0, 0, 1, 0]
        } else if operation == "constraints" {
            [0; 7]
        } else {
            work
        };
        check_work(undo.owner.kernel.calls.take(), solves, undo_work);
        assert_eq!(undo.owner.kernel.inner.population(), union);
        compare(&mut undo)?;
        undo.commit_step()?;
        assert_eq!(live.scene(), &scene);
        assert_eq!(live.session().current().version(), original.version());
        assert_eq!(live.session().current().path(), original.path());
        assert_eq!(live.kernel.inner.population(), old);
        assert_eq!(std::fs::read(&path)?, source_bytes);
        assert_eq!(std::fs::read(original.path())?, bytes);
    }
    Ok(())
}

#[test]
fn session_operations_on_live_counting_kernel() -> Result<()> {
    exercise::<TopologyMock>()
}

#[test]
fn native_session_operations_match_cold_geometry_and_names() -> Result<()> {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    exercise::<OcctKernel>()
}

#[test]
fn rejected_mesh_preserves_accepted_file_scene_and_reusable_base() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("source.fcad");
    let edits = fixture(&path, "fillet_radius")?;
    let ctx = OperationContext::default();
    let mut live = LiveDocument::open(
        DocumentSession::open(&path)?,
        counting::CountingMock::new(),
        |_: &mut Counting<TopologyMock>, _: &[u8]| unreachable!("no imports"),
        TessellationParams::default(),
        &ctx,
    )?;
    let original = live.session().current();
    let scene = live.scene().clone();
    let shapes = live.kernel.inner.live.clone();
    let files = || -> Result<std::collections::BTreeSet<std::ffi::OsString>> {
        Ok(
            std::fs::read_dir(original.path().parent().expect("private directory"))?
                .map(|p| p.map(|entry| entry.file_name()))
                .collect::<std::io::Result<_>>()?,
        )
    };
    let before = files()?;
    live.kernel.inner.refuse_mesh = true;
    let error = live
        .apply(original.version(), edits.clone(), &ctx)
        .err()
        .expect("mesh refused");
    assert!(error.to_string().contains("requested mesh failure"));
    assert_eq!(live.kernel.inner.live, shapes);
    assert_eq!(files()?, before);
    assert_eq!(live.scene(), &scene);
    assert_eq!(live.session().current().version(), original.version());
    assert!(!live.session().can_undo());
    live.kernel.inner.refuse_mesh = false;
    live.kernel.calls.take();
    let solves = ferritecad_sketch_solver::native_solves();
    let mut step = live.apply(original.version(), edits, &ctx)?;
    check_work(
        step.owner.kernel.calls.take(),
        solves,
        [0, 0, 0, 1, 0, 1, 0],
    );
    compare(&mut step)?;
    step.commit_step()?;
    assert_eq!(live.kernel.inner.live.len(), 2);
    Ok(())
}
