// SPDX-License-Identifier: MIT
#![allow(dead_code, clippy::panic)]
use ferritecad_document::*;
use ferritecad_jobs::EditRequest;
use ferritecad_types::{ObjectId, StableEntityId};
use std::path::Path;

pub fn vertex_changes(path: &Path, id: ObjectId, vertices: &[SketchVertex]) -> Vec<EditChange> {
    ferritecad_fixtures::vertex_changes(
        &Document::open_read_only(path).expect("fixture document"),
        id,
        vertices,
    )
    .expect("vertex changes")
}
pub fn constraint_changes(
    path: &Path,
    id: ObjectId,
    edits: &SketchConstraintEdits,
) -> Vec<EditChange> {
    ferritecad_fixtures::constraint_changes(
        &Document::open_read_only(path).expect("fixture document"),
        id,
        edits,
    )
    .expect("constraint changes")
}
pub fn sketch(request: &EditRequest) -> ObjectId {
    request.changes.iter().find_map(|change| match change {
        EditChange::SetCurveGeometry {sketch,..} | EditChange::AddConstraint {sketch,..} | EditChange::RemoveConstraint {sketch,..} => Some(*sketch), _ => None,
    }).unwrap_or_else(|| Document::open_read_only(&request.source).expect("document").objects().expect("objects").into_iter()
        .find(|o| matches!(&o.payload, ObjectPayload::Sketch(s) if s.curves.iter().all(|c| matches!(c.geometry, SketchGeometry::Line{..})))).expect("line sketch").id)
}
pub fn vertices(request: &EditRequest) -> Vec<SketchVertex> {
    let d = Document::open_read_only(&request.source).expect("fixture document");
    let ObjectPayload::Sketch(stored) = d
        .object(sketch(request))
        .expect("object")
        .expect("sketch")
        .payload
    else {
        panic!("sketch")
    };
    vertices_in(request, &stored)
}

/// Inspect the user rules in a form's transaction, excluding automatic closure links.
pub fn constraints(request: &EditRequest) -> SketchConstraintEdits {
    let mut result = SketchConstraintEdits::default();
    let line = |curve, kind| AddSketchConstraint::Line(AddLineConstraint::Line { curve, kind });
    let endpoint = |at| LineEndpoint::of(at).expect("line endpoint");
    let coordinate = |v| SketchCoordinateMm::new(v).expect("coordinate");
    let vector = |s: SketchSegmentRef| {
        DirectedLine::new(s.from.curve, endpoint(s.from.at), endpoint(s.to.at)).expect("vector")
    };
    for change in &request.changes {
        match change {
            EditChange::RemoveConstraint { constraint, .. } => result.remove.push(*constraint),
            EditChange::AddConstraint { constraint, .. } => {
                use SketchConstraintRule as R;
                let added = match &constraint.rule {
                    R::Coincident { a, b }
                        if a.at == SketchPointSelector::Center
                            && b.at == SketchPointSelector::Center =>
                    {
                        AddSketchConstraint::Concentric {
                            a: a.curve,
                            b: b.curve,
                        }
                    }
                    R::Coincident { .. } => continue,
                    R::Horizontal { a, .. } => line(a.curve, LineConstraintKind::Horizontal),
                    R::Vertical { a, .. } => line(a.curve, LineConstraintKind::Vertical),
                    R::Distance { a, distance, .. } => line(
                        a.curve,
                        LineConstraintKind::Distance(
                            LineLengthMm::new(distance.value()).expect("length"),
                        ),
                    ),
                    R::Fixed { point, x, y } if point.at != SketchPointSelector::Center => line(
                        point.curve,
                        LineConstraintKind::Fixed {
                            at: endpoint(point.at),
                            x: coordinate(x.value()),
                            y: coordinate(y.value()),
                        },
                    ),
                    R::Fixed { point, x, y } => AddSketchConstraint::Circle {
                        curve: point.curve,
                        kind: CircleConstraintKind::FixedCenter {
                            x: coordinate(x.value()),
                            y: coordinate(y.value()),
                        },
                    },
                    R::Radius { curve, radius } => AddSketchConstraint::Circle {
                        curve: *curve,
                        kind: CircleConstraintKind::Radius(
                            CircleRadiusMm::new(radius.value()).expect("radius"),
                        ),
                    },
                    R::EqualLength { a, b } => AddLineConstraint::EqualLength {
                        a: a.from.curve,
                        b: b.from.curve,
                    }
                    .into(),
                    R::Parallel { a, b } => AddLineConstraint::Relation {
                        a: a.from.curve,
                        b: b.from.curve,
                        relation: LineRelation::Parallel,
                    }
                    .into(),
                    R::Perpendicular { a, b } => AddLineConstraint::Relation {
                        a: a.from.curve,
                        b: b.from.curve,
                        relation: LineRelation::Perpendicular,
                    }
                    .into(),
                    R::Collinear { a, b } => AddLineConstraint::Relation {
                        a: a.from.curve,
                        b: b.from.curve,
                        relation: LineRelation::Collinear,
                    }
                    .into(),
                    R::DirectedAngle { a, b, angle } => AddLineConstraint::DirectedAngle {
                        a: vector(*a),
                        b: vector(*b),
                        angle: DirectedAngle::from_radians(angle.value()).expect("angle"),
                    }
                    .into(),
                    _ => panic!("form rule"),
                };
                result.add.push(added);
            }
            _ => {}
        }
    }
    result
}
#[derive(Debug, Clone, PartialEq)]
pub struct CircleValues {
    pub curve_id: StableEntityId,
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
}
pub fn circle(request: &EditRequest) -> CircleValues {
    circles(request).remove(0)
}
#[derive(Debug, Clone, PartialEq)]
pub struct AnnulusValues {
    pub outer_curve_id: StableEntityId,
    pub inner_curve_id: StableEntityId,
    pub center_mm: [f64; 2],
    pub outer_radius_mm: f64,
    pub inner_radius_mm: f64,
}
pub fn annulus(request: &EditRequest) -> AnnulusValues {
    let values = circles(request);
    let outer = &values[0];
    let inner = &values[1];
    AnnulusValues {
        outer_curve_id: outer.curve_id,
        inner_curve_id: inner.curve_id,
        center_mm: outer.center_mm,
        outer_radius_mm: outer.radius_mm,
        inner_radius_mm: inner.radius_mm,
    }
}

pub fn solve(
    result: &ferritecad_jobs::EditedDocument,
) -> Option<ferritecad_eval::SketchSolveReport> {
    solve_path(&result.destination)
}
pub fn solve_path(path: &std::path::Path) -> Option<ferritecad_eval::SketchSolveReport> {
    let document = Document::open_read_only(path).expect("published document");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("native kernel");
    let built = ferritecad_eval::rebuild_cold(
        &document,
        &mut kernel,
        &ferritecad_kernel::OperationContext::default(),
    )
    .expect("cold rebuild");
    let report = built.solve_reports().next().map(|(_, r)| r.clone());
    built.release_all(&mut kernel);
    report
}
pub fn set_circle(request: &mut EditRequest, index: usize, center: [f64; 2], radius: f64) {
    let EditChange::SetCurveGeometry { geometry, .. } = &mut request.changes[index] else {
        panic!("curve change")
    };
    *geometry = SketchGeometry::Circle {
        center: Point2 {
            x: center[0],
            y: center[1],
        },
        radius,
    };
}

pub fn assert_meta(a: &DocumentMeta, b: &DocumentMeta) {
    let (mut a, mut b) = (a.clone(), b.clone());
    a.modified_at.clear();
    b.modified_at.clear();
    assert_eq!(a, b);
}

pub fn vertices_in(request: &EditRequest, stored: &Sketch) -> Vec<SketchVertex> {
    stored
        .curves
        .iter()
        .map(|curve| {
            let geometry = request
                .changes
                .iter()
                .rev()
                .find_map(|change| match change {
                    EditChange::SetCurveGeometry {
                        curve: id,
                        geometry,
                        ..
                    } if *id == curve.id => Some(geometry),
                    _ => None,
                })
                .unwrap_or(&curve.geometry);
            let SketchGeometry::Line { start, .. } = geometry else {
                panic!("line")
            };
            SketchVertex {
                curve_id: curve.id,
                start_mm: [start.x, start.y],
            }
        })
        .collect()
}
fn circles(request: &EditRequest) -> Vec<CircleValues> {
    let d = Document::open_read_only(&request.source).expect("fixture document");
    let id = request
        .changes
        .first()
        .map(EditChange::object)
        .unwrap_or_else(|| ferritecad_fixtures::sketch(&d).expect("sketch").0);
    let ObjectPayload::Sketch(s) = d.object(id).expect("object").expect("sketch").payload else {
        panic!("sketch")
    };
    let mut curves: Vec<_> = s
        .curves
        .iter()
        .filter(|c| matches!(c.geometry, SketchGeometry::Circle { .. }))
        .collect();
    curves.sort_by(|a, b| {
        let radius = |c: &&SketchCurve| {
            if let SketchGeometry::Circle { radius, .. } = c.geometry {
                radius
            } else {
                0.
            }
        };
        radius(b).total_cmp(&radius(a))
    });
    curves
        .into_iter()
        .map(|c| {
            let geometry = request
                .changes
                .iter()
                .rev()
                .find_map(|change| match change {
                    EditChange::SetCurveGeometry {
                        curve, geometry, ..
                    } if *curve == c.id => Some(geometry),
                    _ => None,
                })
                .unwrap_or(&c.geometry);
            let SketchGeometry::Circle { center, radius } = geometry else {
                panic!("circle")
            };
            CircleValues {
                curve_id: c.id,
                center_mm: [center.x, center.y],
                radius_mm: *radius,
            }
        })
        .collect()
}

pub fn assert_refused(request: &EditRequest, names: &[&str]) {
    if !ferritecad_fixtures::native_dependencies(&[(ferritecad_occt::is_available(), "OCCT")]) {
        return;
    }
    let bytes = std::fs::read(&request.source).expect("source");
    let root = tempfile::tempdir().expect("destination");
    let mut request = request.clone();
    request.destination = root.path().join("refused.fcad");
    let mut kernel = ferritecad_occt::OcctKernel::new().expect("kernel");
    let error = ferritecad_jobs::edit_copy(&request, &mut kernel, &Default::default())
        .expect_err("rebuild refusal")
        .to_string();
    for name in names {
        assert!(error.contains(name), "{error}");
    }
    assert!(!request.destination.exists());
    assert_eq!(std::fs::read(&request.source).expect("source"), bytes);
}
