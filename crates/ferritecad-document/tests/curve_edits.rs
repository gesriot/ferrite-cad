// SPDX-License-Identifier: MIT
#![allow(clippy::panic)]
#[path = "../../ferritecad-fixtures/tests/support/curve_edits.rs"]
mod support;
use ferritecad_document::*;
use ferritecad_types::StableEntityId;
use support::*;

#[test]
fn additions_round_trip_every_geometry_and_preserve_order() -> TestResult {
    let mut f = Drawing::new(vec![], false);
    let mut guide = line([20., 0.], [20., 8.]);
    guide.construction = true;
    let curves = vec![
        line([0., 0.], [10., 0.]),
        curve(SketchGeometry::Circle {
            center: point(4., 4.),
            radius: 2.,
        }),
        curve(SketchGeometry::Arc {
            center: point(1., 2.),
            radius: 3.,
            start_angle: 1.,
            end_angle: -1.,
        }),
        curve(SketchGeometry::Point { at: point(2., 4.) }),
        guide,
    ];
    let changes: Vec<_> = curves
        .iter()
        .map(|c| EditChange::AddCurve {
            sketch: f.sketch,
            curve: c.clone(),
        })
        .collect();
    f.doc.write_changes(&changes)?;
    assert_eq!(f.saved().curves, curves);
    let reopened = Document::open_read_only(&f.path)?;
    assert_eq!(
        reopened.object(f.sketch)?.expect("sketch").payload,
        ObjectPayload::Sketch(f.saved())
    );
    Ok(())
}

#[test]
fn curve_edits_refuse_occupied_missing_and_wrong_kind_without_writes() -> TestResult {
    let mut f = Drawing::new(rectangle(), false);
    let saved = f.saved();
    let absent = StableEntityId::new();
    let cases = [
        (
            EditChange::AddCurve {
                sketch: f.sketch,
                curve: saved.curves[0].clone(),
            },
            format!("invalid input: curve {} already exists", saved.curves[0].id),
        ),
        (
            EditChange::RemoveCurve {
                sketch: f.sketch,
                curve: absent,
            },
            format!("invalid input: sketch has no curve {absent}"),
        ),
        (
            EditChange::SetConstruction {
                sketch: f.sketch,
                curve: absent,
                construction: true,
            },
            format!("invalid input: sketch has no curve {absent}"),
        ),
    ];
    let bytes = std::fs::read(&f.path)?;
    let version = f.doc.content_version()?;
    for (change, expected) in cases {
        assert_eq!(
            f.doc
                .write_changes(&[change])
                .expect_err("refusal")
                .to_string(),
            expected
        );
        assert_eq!(f.doc.content_version()?, version);
        assert_eq!(std::fs::read(&f.path)?, bytes);
    }
    for change in [
        EditChange::AddCurve {
            sketch: f.plane,
            curve: saved.curves[0].clone(),
        },
        EditChange::RemoveCurve {
            sketch: f.plane,
            curve: absent,
        },
        EditChange::SetConstruction {
            sketch: f.plane,
            curve: absent,
            construction: true,
        },
    ] {
        assert_eq!(
            f.doc
                .write_changes(&[change])
                .expect_err("wrong kind")
                .to_string(),
            format!(
                "invalid input: datum plane 'XY' ({}) is not a Sketch; only a Sketch has \
                 editable curves.",
                f.plane
            )
        );
        assert_eq!(f.doc.content_version()?, version);
        assert_eq!(std::fs::read(&f.path)?, bytes);
    }
    Ok(())
}

#[test]
fn constraints_are_checked_once_after_the_complete_curve_edit_batch() -> TestResult {
    for constraint_first in [true, false] {
        let circle = curve(SketchGeometry::Circle {
            center: point(0., 0.),
            radius: 2.,
        });
        let mut f = Drawing::new(vec![circle.clone()], false);
        let constraint = SketchConstraint {
            id: StableEntityId::new(),
            rule: SketchConstraintRule::Radius {
                curve: circle.id,
                radius: Expression::constant(2.)?,
            },
        };
        f.doc.write_changes(&[EditChange::AddConstraint {
            sketch: f.sketch,
            constraint: constraint.clone(),
        }])?;
        let bytes = std::fs::read(&f.path)?;
        let version = f.doc.content_version()?;
        let remove = EditChange::RemoveCurve {
            sketch: f.sketch,
            curve: circle.id,
        };
        let error = f
            .doc
            .write_changes(std::slice::from_ref(&remove))
            .expect_err("saved constraint");
        assert_eq!(
            error.to_string(),
            format!(
                "invalid input: constraint {} refers to curve {}, which is not a curve of \
                 this sketch",
                constraint.id, circle.id
            )
        );
        assert_eq!(f.doc.content_version()?, version);
        assert_eq!(std::fs::read(&f.path)?, bytes);
        let mut changes = vec![
            remove,
            EditChange::RemoveConstraint {
                sketch: f.sketch,
                constraint: constraint.id,
            },
        ];
        if constraint_first {
            changes.reverse();
        }
        f.doc.write_changes(&changes)?;
        assert!(f.saved().curves.is_empty());
        assert!(f.saved().constraints.is_empty());
    }
    Ok(())
}

#[test]
fn remove_then_add_reuses_identity_and_construction_preserves_other_curves() -> TestResult {
    let mut f = Drawing::new(rectangle(), false);
    let saved = f.saved();
    let mut replacement = saved.curves[1].clone();
    replacement.geometry = SketchGeometry::Circle {
        center: point(2., 3.),
        radius: 1.,
    };
    f.doc.write_changes(&[
        EditChange::RemoveCurve {
            sketch: f.sketch,
            curve: replacement.id,
        },
        EditChange::AddCurve {
            sketch: f.sketch,
            curve: replacement.clone(),
        },
        EditChange::SetConstruction {
            sketch: f.sketch,
            curve: replacement.id,
            construction: true,
        },
    ])?;
    replacement.construction = true;
    assert_eq!(
        f.saved().curves,
        vec![
            saved.curves[0].clone(),
            saved.curves[2].clone(),
            saved.curves[3].clone(),
            replacement.clone()
        ]
    );
    let version = f.doc.model_version()?;
    f.doc.write_changes(&[EditChange::SetConstruction {
        sketch: f.sketch,
        curve: replacement.id,
        construction: true,
    }])?;
    assert_eq!(f.doc.model_version()?, version);
    Ok(())
}
