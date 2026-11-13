// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
#[path = "../../ferritecad-fixtures/tests/support/curve_edits.rs"]
mod support;
use ferritecad_document::*;
use ferritecad_jobs::{DocumentSession, EditRequest, HistoryLimits, LiveDocument, edit_copy};
use ferritecad_kernel::{OperationContext, TessellationParams, mock::MockKernel};
use ferritecad_types::ObjectId;
use support::*;

#[test]
fn adding_all_curve_kinds_is_one_undo_and_same_construction_is_no_change() -> TestResult {
    let f = Drawing::new(vec![], false);
    let context = OperationContext::default();
    let sessions = tempfile::tempdir()?;
    let mut live = LiveDocument::open(
        DocumentSession::open_in(sessions.path(), &f.path, HistoryLimits::default())?,
        MockKernel::new(),
        |_, _| unreachable!("no imports"),
        TessellationParams::default(),
        &context,
    )?;
    let before = std::fs::read(live.session().current().path())?;
    let curves = vec![
        line([0., 0.], [10., 0.]),
        curve(SketchGeometry::Circle {
            center: point(2., 3.),
            radius: 2.,
        }),
        curve(SketchGeometry::Arc {
            center: point(0., 0.),
            radius: 1.,
            start_angle: 1.,
            end_angle: -1.,
        }),
        curve(SketchGeometry::Point { at: point(0., 0.) }),
        SketchCurve {
            construction: true,
            ..line([1., 0.], [1., 5.])
        },
    ];
    let changes = curves
        .iter()
        .map(|curve| EditChange::AddCurve {
            sketch: f.sketch,
            curve: curve.clone(),
        })
        .collect();
    let version = live.session().current().version();
    assert_eq!(
        live.apply(version, changes, &context)?.commit_step()?,
        ferritecad_jobs::StepCommit::Accepted
    );
    assert!(live.scene().rebuild.all_built());
    let doc = Document::open_read_only(live.session().current().path())?;
    let ObjectPayload::Sketch(saved) = doc.object(f.sketch)?.expect("sketch").payload else {
        unreachable!()
    };
    assert_eq!(saved.curves, curves);
    let version = live.session().current().version();
    assert_eq!(
        live.apply(
            version,
            vec![EditChange::SetConstruction {
                sketch: f.sketch,
                curve: curves[4].id,
                construction: true
            }],
            &context
        )?
        .commit_step()?,
        ferritecad_jobs::StepCommit::NoChange
    );
    live.undo(&context)?.commit_step()?;
    assert_eq!(std::fs::read(live.session().current().path())?, before);
    assert!(!live.session().can_undo());
    Ok(())
}
fn refusal(f: &Drawing, changes: Vec<EditChange>, expected: String) -> TestResult {
    refusal_with(f, changes, expected, &mut MockKernel::new())
}
fn refusal_with(
    f: &Drawing,
    changes: Vec<EditChange>,
    expected: String,
    kernel: &mut impl ferritecad_kernel::GeometryKernel,
) -> TestResult {
    let baseline = ferritecad_eval::rebuild_cold(&f.doc, kernel, &OperationContext::default())?;
    assert!(baseline.all_built(), "baseline: {:?}", baseline.report());
    baseline.release_all(kernel);
    let bytes = std::fs::read(&f.path)?;
    let version = DocumentVersion {
        document_id: f.doc.meta().document_id,
        content: f.doc.content_version()?,
    };
    let destination = f.root.path().join("candidate.fcad");
    let error = edit_copy(
        &EditRequest {
            source: f.path.clone(),
            destination: destination.clone(),
            expected: version,
            changes,
        },
        kernel,
        &OperationContext::default(),
    )
    .expect_err("atomic refusal");
    assert_eq!(error.to_string(), expected);
    assert_eq!(std::fs::read(&f.path)?, bytes);
    assert_eq!(f.doc.content_version()?, version.content);
    assert!(!destination.exists());
    Ok(())
}
fn unbuilt(f: &Drawing, reason: &str) -> String {
    format!(
        "Extrude ({}), Part ({}): edit would leave features unbuilt:\n\
         {}: {reason}\n{}: not built: depends on {}: {reason}\n",
        f.feature, f.body, f.feature, f.body, f.feature
    )
}
#[test]
fn making_a_consumed_boundary_construction_refuses_all_new_failures() -> TestResult {
    let curves = rectangle();
    let removed = curves[0].id;
    let open = curves[1].id;
    let f = Drawing::new(curves, true);
    let reason = format!(
        "invalid input: sketch {} gives no profile: invalid input: the profile does not \
         close: curve {open} has a free start at (10, 0)",
        f.sketch
    );
    refusal(
        &f,
        vec![EditChange::SetConstruction {
            sketch: f.sketch,
            curve: removed,
            construction: true,
        }],
        unbuilt(&f, &reason),
    )
}
#[test]
fn removing_a_consumed_boundary_refuses_all_new_failures() -> TestResult {
    let curves = rectangle();
    let removed = curves[0].id;
    let open = curves[1].id;
    let f = Drawing::new(curves, true);
    let reason = format!(
        "invalid input: sketch {} gives no profile: invalid input: the profile does not \
         close: curve {open} has a free start at (10, 0)",
        f.sketch
    );
    refusal(
        &f,
        vec![EditChange::RemoveCurve {
            sketch: f.sketch,
            curve: removed,
        }],
        unbuilt(&f, &reason),
    )
}
#[test]
fn unconsumed_drawings_accept_removal_and_construction_changes() -> TestResult {
    for construction in [false, true] {
        let f = Drawing::new(rectangle(), false);
        let curve = f.saved().curves[0].id;
        let change = if construction {
            EditChange::SetConstruction {
                sketch: f.sketch,
                curve,
                construction: true,
            }
        } else {
            EditChange::RemoveCurve {
                sketch: f.sketch,
                curve,
            }
        };
        let out = f.root.path().join("accepted.fcad");
        let result = edit_copy(
            &EditRequest {
                source: f.path.clone(),
                destination: out,
                expected: DocumentVersion {
                    document_id: f.doc.meta().document_id,
                    content: f.doc.content_version()?,
                },
                changes: vec![change],
            },
            &mut MockKernel::new(),
            &OperationContext::default(),
        )?;
        assert!(result.rebuild.all_built());
    }
    Ok(())
}
#[test]
fn removed_region_identity_is_refused_by_existing_profile_selection() -> TestResult {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let a = curve(SketchGeometry::Circle {
        center: point(0., 0.),
        radius: 2.,
    });
    let b = curve(SketchGeometry::Circle {
        center: point(10., 0.),
        radius: 2.,
    });
    let mut f = Drawing::new(vec![a.clone(), b], true);
    f.doc.write_changes(&[EditChange::SetRegions {
        object: f.feature,
        regions: vec![a.id],
    }])?;
    let reason = format!(
        "invalid input: region curve {} is missing or is construction geometry",
        a.id
    );
    refusal_with(
        &f,
        vec![EditChange::RemoveCurve {
            sketch: f.sketch,
            curve: a.id,
        }],
        unbuilt(&f, &reason),
        &mut ferritecad_occt::OcctKernel::new()?,
    )
}
#[test]
fn empty_sketch_addition_builds_and_its_new_extrusion_refuses() -> TestResult {
    let mut f = Drawing::new(vec![], false);
    let added = ObjectId::new();
    f.doc.write_changes(&[EditChange::AddSketch {
        object: added,
        name: Some("Empty".into()),
        sketch: Sketch {
            plane: f.plane,
            curves: vec![],
            constraints: vec![],
        },
    }])?;
    let built = ferritecad_eval::rebuild_cold(
        &f.doc,
        &mut MockKernel::new(),
        &OperationContext::default(),
    )?;
    assert!(built.all_built());
    assert!(
        built
            .sketch_presentation(added)
            .expect("empty drawing")
            .curves()
            .is_empty()
    );
    put(
        &mut f.doc,
        f.body,
        "Part",
        ObjectPayload::Body(Body {
            tip_feature: None,
            rollback: None,
        }),
    );
    let reason = format!(
        "invalid input: sketch {added} gives no profile: invalid input: the sketch has no \
         main Line, Arc or Circle, so it gives no profile"
    );
    let expected = format!(
        "Part ({}), Extrude ({}): edit would leave features unbuilt:\n\
         {}: {reason}\n{}: not built: depends on {}: {reason}\n",
        f.body, f.feature, f.feature, f.body, f.feature
    );
    refusal(
        &f,
        vec![EditChange::AddFeature {
            object: f.feature,
            body: f.body,
            name: Some("Extrude".into()),
            feature: FeatureAddition::Extrude {
                profile: added,
                regions: vec![],
                reversed: false,
                operation: SolidOperation::NewBody,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(3.)?,
                },
            },
        }],
        expected,
    )
}

#[test]
fn removing_a_revolve_axis_uses_the_existing_document_refusal() -> TestResult {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let mut curves = rectangle();
    for c in &mut curves {
        if let SketchGeometry::Line { start, end } = &mut c.geometry {
            start.x += 2.;
            end.x += 2.;
        }
    }
    let mut axis = line([0., -5.], [0., 15.]);
    axis.construction = true;
    curves.push(axis.clone());
    let mut f = Drawing::new(curves, true);
    put(
        &mut f.doc,
        f.feature,
        "Extrude",
        ObjectPayload::Revolve(Revolve {
            profile: f.sketch,
            regions: vec![],
            axis: RevolveAxis::Line(axis.id),
            extent: RevolveExtent::FullTurn,
            operation: SolidOperation::NewBody,
            previous: None,
        }),
    );
    let expected = format!(
        "invalid input: document validation failed before cold rebuild: revolve.axis: revolve \
         axis {} is not a Line of its profile sketch",
        axis.id
    );
    refusal_with(
        &f,
        vec![EditChange::RemoveCurve {
            sketch: f.sketch,
            curve: axis.id,
        }],
        expected,
        &mut ferritecad_occt::OcctKernel::new()?,
    )
}
#[test]
fn replacing_a_curve_identity_refuses_its_fillet_edge_without_retargeting() -> TestResult {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return Ok(());
    }
    let curves = rectangle();
    let mut f = Drawing::new(curves.clone(), true);
    let edge = ferritecad_fixtures::feature_changes::extruded_fixture_edge(
        f.feature,
        f.plane,
        curves[3].id,
        curves[0].id,
    );
    let fillet = ObjectId::new();
    f.doc.write_changes(&[EditChange::AddFeature {
        object: fillet,
        body: f.body,
        name: Some("Fillet".into()),
        feature: FeatureAddition::Fillet {
            edges: vec![edge],
            radius_mm: Expression::constant(0.5)?,
        },
    }])?;
    let mut replacement = curves[0].clone();
    replacement.id = ferritecad_types::StableEntityId::new();
    let reason = format!(
        "unresolved topology reference: feature {fillet}, reference {}: unknown name: the \
         producer minted no name with this digest",
        edge.id
    );
    let expected = format!(
        "Part ({}), Fillet ({fillet}): edit would leave features unbuilt:\n{fillet}: \
         {reason}\n{}: not built: depends on {fillet}: {reason}\n",
        f.body, f.body
    );
    refusal_with(
        &f,
        vec![
            EditChange::RemoveCurve {
                sketch: f.sketch,
                curve: curves[0].id,
            },
            EditChange::AddCurve {
                sketch: f.sketch,
                curve: replacement,
            },
        ],
        expected,
        &mut ferritecad_occt::OcctKernel::new()?,
    )
}
