// SPDX-License-Identifier: MIT
//! Read-only projections used by widget and independent geometry assertions.
#![allow(clippy::panic)]
use crate::feature_facts::{CircularCut, CutExtent, EdgeChamfer, EdgeFillet};
use ferritecad_document::{
    AddLineConstraint, AddSketchConstraint, EditChange, EndCondition, FeatureAddition,
    LineConstraintKind, ObjectPayload, SketchGeometry,
};
use ferritecad_jobs::EditRequest;
use ferritecad_types::{ObjectId, StableEntityId};

/// A U with two disjoint, parallel rims at different heights: the right one
/// (Line 1) at y 22 and the left one (Line 5) at y 20. Neither touches the
/// other and no horizontal or vertical is stored, so only a collinearity can
/// bring them onto one line.
pub(crate) const U_PROFILE: [[f64; 2]; 8] = [
    [50., 22.],
    [35., 22.],
    [35., 8.],
    [15., 8.],
    [15., 20.],
    [0., 20.],
    [0., 0.],
    [50., 0.],
];

pub(crate) trait RequestFacts {
    fn addition(&self) -> (ObjectId, ObjectId, &FeatureAddition);
    fn fillet(&self) -> EdgeFillet;
    fn chamfer(&self) -> EdgeChamfer;
    fn circle(&self) -> CircularCut;
    fn cut_id(&self) -> ObjectId;
    fn curve_id(&self) -> ferritecad_types::StableEntityId;
}
impl RequestFacts for EditRequest {
    fn addition(&self) -> (ObjectId, ObjectId, &FeatureAddition) {
        self.changes
            .iter()
            .find_map(|c| match c {
                EditChange::AddFeature {
                    object,
                    body,
                    feature,
                    ..
                } => Some((*object, *body, feature)),
                EditChange::AddParameter { .. }
                | EditChange::SetParameter { .. }
                | EditChange::RenameParameter { .. }
                | EditChange::RemoveObject { .. }
                | EditChange::SetSuppressed { .. }
                | EditChange::SetRollback { .. }
                | EditChange::SetExpressionText { .. }
                | EditChange::SetKeepSelection { .. }
                | EditChange::AddDatumPlane { .. }
                | EditChange::SetDatumFace { .. }
                | EditChange::AddBody { .. }
                | EditChange::SetRegions { .. }
                | EditChange::SetOperation { .. }
                | EditChange::SetReversed { .. }
                | EditChange::SetRevolveAxis { .. }
                | EditChange::SetRevolveExtent { .. }
                | EditChange::AddSketch { .. }
                | EditChange::SetEdges { .. }
                | EditChange::SetEndCondition { .. }
                | EditChange::SetExpression { .. }
                | EditChange::AddCurve { .. }
                | EditChange::RemoveCurve { .. }
                | EditChange::SetConstruction { .. }
                | EditChange::SetCurveGeometry { .. }
                | EditChange::AddConstraint { .. }
                | EditChange::RemoveConstraint { .. } => None,
            })
            .expect("feature addition")
    }
    fn fillet(&self) -> EdgeFillet {
        let FeatureAddition::Fillet { edges, radius_mm } = self.addition().2 else {
            panic!("Fillet request")
        };
        EdgeFillet {
            edges: edges.clone(),
            radius_mm: radius_mm.value(),
        }
    }
    fn chamfer(&self) -> EdgeChamfer {
        let FeatureAddition::Chamfer { edges, distance_mm } = self.addition().2 else {
            panic!("Chamfer request")
        };
        EdgeChamfer {
            edges: edges.clone(),
            distance_mm: distance_mm.value(),
        }
    }
    fn circle(&self) -> CircularCut {
        let geometry = self
            .changes
            .iter()
            .find_map(|c| match c {
                EditChange::SetCurveGeometry { geometry, .. } => Some(geometry),
                EditChange::AddSketch { sketch, .. } => Some(&sketch.curves[0].geometry),
                EditChange::AddParameter { .. }
                | EditChange::SetParameter { .. }
                | EditChange::RenameParameter { .. }
                | EditChange::RemoveObject { .. }
                | EditChange::SetSuppressed { .. }
                | EditChange::SetRollback { .. }
                | EditChange::SetExpressionText { .. }
                | EditChange::SetKeepSelection { .. }
                | EditChange::AddDatumPlane { .. }
                | EditChange::SetDatumFace { .. }
                | EditChange::AddBody { .. }
                | EditChange::SetRegions { .. }
                | EditChange::SetOperation { .. }
                | EditChange::SetReversed { .. }
                | EditChange::SetRevolveAxis { .. }
                | EditChange::SetRevolveExtent { .. }
                | EditChange::AddFeature { .. }
                | EditChange::SetEdges { .. }
                | EditChange::SetEndCondition { .. }
                | EditChange::SetExpression { .. }
                | EditChange::AddCurve { .. }
                | EditChange::RemoveCurve { .. }
                | EditChange::SetConstruction { .. }
                | EditChange::AddConstraint { .. }
                | EditChange::RemoveConstraint { .. } => None,
            })
            .expect("circle geometry");
        let SketchGeometry::Circle { center, radius } = geometry else {
            panic!("Circle")
        };
        let end = self
            .changes
            .iter()
            .find_map(|c| match c {
                EditChange::SetEndCondition { end_condition, .. } => Some(end_condition),
                EditChange::AddFeature {
                    feature:
                        FeatureAddition::Extrude {
                            operation: ferritecad_document::SolidOperation::Cut,
                            end_condition,
                            ..
                        },
                    ..
                } => Some(end_condition),
                EditChange::AddFeature { .. }
                | EditChange::AddParameter { .. }
                | EditChange::SetParameter { .. }
                | EditChange::RenameParameter { .. }
                | EditChange::RemoveObject { .. }
                | EditChange::SetSuppressed { .. }
                | EditChange::SetRollback { .. }
                | EditChange::SetExpressionText { .. }
                | EditChange::SetKeepSelection { .. }
                | EditChange::AddDatumPlane { .. }
                | EditChange::SetDatumFace { .. }
                | EditChange::AddBody { .. }
                | EditChange::SetRegions { .. }
                | EditChange::SetOperation { .. }
                | EditChange::SetReversed { .. }
                | EditChange::SetRevolveAxis { .. }
                | EditChange::SetRevolveExtent { .. }
                | EditChange::AddSketch { .. }
                | EditChange::SetEdges { .. }
                | EditChange::SetExpression { .. }
                | EditChange::AddCurve { .. }
                | EditChange::RemoveCurve { .. }
                | EditChange::SetConstruction { .. }
                | EditChange::SetCurveGeometry { .. }
                | EditChange::AddConstraint { .. }
                | EditChange::RemoveConstraint { .. } => None,
            })
            .expect("Cut end");
        CircularCut {
            center_mm: [center.x, center.y],
            radius_mm: *radius,
            extent: match end {
                EndCondition::Blind { distance } => CutExtent::Blind {
                    depth_mm: distance.value(),
                },
                EndCondition::ThroughAll => CutExtent::ThroughAll,
                _ => panic!("Cut end"),
            },
        }
    }
    fn cut_id(&self) -> ObjectId {
        self.changes
            .iter()
            .find_map(|c| match c {
                EditChange::SetEndCondition { object, .. }
                | EditChange::AddFeature { object, .. } => Some(*object),
                EditChange::AddParameter { .. }
                | EditChange::SetParameter { .. }
                | EditChange::RenameParameter { .. }
                | EditChange::RemoveObject { .. }
                | EditChange::SetSuppressed { .. }
                | EditChange::SetRollback { .. }
                | EditChange::SetExpressionText { .. }
                | EditChange::SetKeepSelection { .. }
                | EditChange::AddDatumPlane { .. }
                | EditChange::SetDatumFace { .. }
                | EditChange::AddBody { .. }
                | EditChange::SetRegions { .. }
                | EditChange::SetOperation { .. }
                | EditChange::SetReversed { .. }
                | EditChange::SetRevolveAxis { .. }
                | EditChange::SetRevolveExtent { .. }
                | EditChange::AddSketch { .. }
                | EditChange::SetEdges { .. }
                | EditChange::SetExpression { .. }
                | EditChange::AddCurve { .. }
                | EditChange::RemoveCurve { .. }
                | EditChange::SetConstruction { .. }
                | EditChange::SetCurveGeometry { .. }
                | EditChange::AddConstraint { .. }
                | EditChange::RemoveConstraint { .. } => None,
            })
            .expect("Cut id")
    }
    fn curve_id(&self) -> ferritecad_types::StableEntityId {
        self.changes
            .iter()
            .find_map(|c| match c {
                EditChange::SetCurveGeometry { curve, .. } => Some(*curve),
                EditChange::AddSketch { sketch, .. } => Some(sketch.curves[0].id),
                EditChange::AddParameter { .. }
                | EditChange::SetParameter { .. }
                | EditChange::RenameParameter { .. }
                | EditChange::RemoveObject { .. }
                | EditChange::SetSuppressed { .. }
                | EditChange::SetRollback { .. }
                | EditChange::SetExpressionText { .. }
                | EditChange::SetKeepSelection { .. }
                | EditChange::AddDatumPlane { .. }
                | EditChange::SetDatumFace { .. }
                | EditChange::AddBody { .. }
                | EditChange::SetRegions { .. }
                | EditChange::SetOperation { .. }
                | EditChange::SetReversed { .. }
                | EditChange::SetRevolveAxis { .. }
                | EditChange::SetRevolveExtent { .. }
                | EditChange::AddFeature { .. }
                | EditChange::SetEdges { .. }
                | EditChange::SetEndCondition { .. }
                | EditChange::SetExpression { .. }
                | EditChange::AddCurve { .. }
                | EditChange::RemoveCurve { .. }
                | EditChange::SetConstruction { .. }
                | EditChange::AddConstraint { .. }
                | EditChange::RemoveConstraint { .. } => None,
            })
            .expect("curve id")
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FixtureCorner {
    pub reference: ferritecad_document::EntityReference,
    pub joint: [ferritecad_types::StableEntityId; 2],
    pub corner_mm: [f64; 2],
    pub adjacent_lengths_mm: [f64; 2],
}
#[derive(Debug, Clone)]
pub(crate) struct FixtureTarget {
    pub target: crate::feature_facts::EdgeTarget,
    pub base_feature: ObjectId,
    pub profile: ObjectId,
    pub constrained: bool,
    pub corners: Vec<FixtureCorner>,
}
impl std::ops::Deref for FixtureTarget {
    type Target = crate::feature_facts::EdgeTarget;
    fn deref(&self) -> &Self::Target {
        &self.target
    }
}
// These are the declared inputs of the rectangular test fixtures, used by their
// independent geometry oracle. The window gets its references from the scene.
pub(crate) fn fixture_target(reading: &crate::sketch_facts::Source) -> FixtureTarget {
    let target = reading.fillet_bodies[0].target.as_ref().expect("target");
    let (base, profile) = reading
        .objects
        .iter()
        .find_map(|o| match &o.payload {
            ObjectPayload::Extrude(e) if e.previous.is_none() => Some((o.id, e.profile)),
            _ => None,
        })
        .expect("fixture extrusion");
    let ObjectPayload::Sketch(sketch) = &reading
        .objects
        .iter()
        .find(|o| o.id == profile)
        .expect("profile")
        .payload
    else {
        panic!("Sketch")
    };
    let mut corners = Vec::new();
    let count = sketch.curves.len();
    for index in 0..count {
        let before = &sketch.curves[(index + count - 1) % count];
        let after = &sketch.curves[index];
        let SketchGeometry::Line { start, end } = after.geometry else {
            panic!("fixture line")
        };
        let SketchGeometry::Line { start: a, end: b } = before.geometry else {
            panic!("fixture line")
        };
        corners.push(FixtureCorner {
            reference: ferritecad_fixtures::feature_changes::extruded_fixture_edge(
                base,
                sketch.plane,
                before.id,
                after.id,
            ),
            joint: ferritecad_fixtures::ordered_labels(before.id, after.id),
            corner_mm: [start.x, start.y],
            adjacent_lengths_mm: [
                (b.x - a.x).hypot(b.y - a.y),
                (end.x - start.x).hypot(end.y - start.y),
            ],
        });
    }
    FixtureTarget {
        target: target.clone(),
        base_feature: base,
        profile,
        constrained: !sketch.constraints.is_empty(),
        corners,
    }
}
pub(crate) fn fixture_corner(reading: &crate::sketch_facts::Source, at: [f64; 2]) -> FixtureCorner {
    fixture_target(reading)
        .corners
        .into_iter()
        .find(|c| c.corner_mm == at)
        .expect("declared fixture corner")
}
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EdgeObservation {
    pub previous: ObjectId,
    pub constrained: bool,
    pub feature: ObjectId,
    pub body: ObjectId,
    pub base_feature: ObjectId,
    pub profile: ObjectId,
    pub edge: ferritecad_document::EntityReference,
    pub joint: [ferritecad_types::StableEntityId; 2],
    pub corner: FixtureCorner,
    pub radius_mm: f64,
    pub distance_mm: f64,
}
pub(crate) fn edge_history(path: &std::path::Path, chamfer: bool) -> Vec<EdgeObservation> {
    let reading = crate::sketch_facts::read(path).expect("reading");
    let target = fixture_target(&reading);
    let mut result = Vec::new();
    let mut previous = target.base_feature;
    while let Some((id, edge, amount)) = reading.objects.iter().find_map(|o| match &o.payload {
        ObjectPayload::Fillet(f) if !chamfer && f.previous == previous => {
            Some((o.id, f.edges[0], f.radius_mm.value()))
        }
        ObjectPayload::Chamfer(c) if chamfer && c.previous == previous => {
            Some((o.id, c.edges[0], c.distance_mm.value()))
        }
        _ => None,
    }) {
        let corner = *target
            .corners
            .iter()
            .find(|c| c.reference.meaning == edge.meaning)
            .expect("fixture reference");
        result.push(EdgeObservation {
            previous,
            constrained: target.constrained,
            feature: id,
            body: target.body,
            base_feature: target.base_feature,
            profile: target.profile,
            edge,
            joint: corner.joint,
            corner,
            radius_mm: amount,
            distance_mm: amount,
        });
        previous = id;
    }
    result
}
pub(crate) fn area(points: &[[f64; 2]]) -> f64 {
    (0..points.len())
        .map(|i| {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f64>()
        .abs()
        / 2.
}
pub(crate) fn reach(end: CutExtent, height: f64) -> f64 {
    match end {
        CutExtent::Blind { depth_mm } => depth_mm.min(height),
        CutExtent::ThroughAll => height,
    }
}

/// One analytic circle (`radii` of one) or two concentric ones (the boundary
/// first, or the bore first when `swapped`), written through the document API: a
/// build with no kernel has no `create-*` command, and these tests need none.
pub(crate) fn write_round(
    path: &std::path::Path,
    center: [f64; 2],
    radii: &[f64],
    height: f64,
    swapped: bool,
) {
    use ferritecad_document::{
        Body, DatumPlane, Dependency, DependencyRole, Document, EndCondition, Expression, Extrude,
        ObjectPayload, Point2, Sketch, SketchCurve, SketchGeometry, SolidOperation,
    };
    use ferritecad_types::{ObjectId, StableEntityId, Transform};
    let mut d = Document::create(path).expect("document");
    let [plane, sketch, extrude, body] = std::array::from_fn(|_| ObjectId::new());
    let drawn: Vec<StableEntityId> = radii.iter().map(|_| StableEntityId::new()).collect();
    d.write(|w| {
        w.put_object(
            plane,
            None,
            0,
            Some("XY"),
            &ObjectPayload::DatumPlane(DatumPlane::Fixed {
                placement: Transform::IDENTITY,
            }),
        )?;
        let mut curves = Vec::new();
        for (id, radius) in drawn.iter().zip(radii) {
            curves.push(SketchCurve {
                id: *id,
                construction: false,
                geometry: SketchGeometry::Circle {
                    center: Point2::new(center[0], center[1])?,
                    radius: *radius,
                },
            });
        }
        if swapped {
            curves.reverse();
        }
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
            extrude,
            None,
            2,
            Some("Extrude1"),
            &ObjectPayload::Extrude(Extrude {
                regions: Vec::new(),
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(height)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                previous: None,
            }),
        )?;
        w.put_object(
            body,
            None,
            3,
            Some("Body"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(extrude),
                rollback: None,
            }),
        )?;
        for (dependent, dependency, role) in [
            (sketch, plane, DependencyRole::Plane),
            (extrude, sketch, DependencyRole::Profile),
            (body, extrude, DependencyRole::BodyTip),
        ] {
            w.add_dependency(Dependency {
                dependent,
                dependency,
                role,
            })?;
        }

        Ok(())
    })
    .expect("round document");
    d.close().expect("close");
}

/// The signed volume, bounds and edge pairing of a binary STL, read here and not
/// by the code that wrote it.
pub(crate) fn read_stl(bytes: &[u8]) -> (Vec<[[f64; 3]; 3]>, f64) {
    let count = u32::from_le_bytes(bytes[80..84].try_into().expect("count")) as usize;
    assert_eq!(bytes.len(), 84 + 50 * count);
    let mut faces = Vec::new();
    let mut six = 0.;
    for t in bytes[84..].as_chunks::<50>().0 {
        let mut p = [[0.; 3]; 3];
        for (i, point) in p.iter_mut().enumerate() {
            for (j, coordinate) in point.iter_mut().enumerate() {
                let k = 12 + 12 * i + 4 * j;
                *coordinate = f64::from(f32::from_le_bytes(t[k..k + 4].try_into().expect("f32")));
                assert!(coordinate.is_finite());
            }
        }
        let [a, b, c] = p;
        six += a[0] * (b[1] * c[2] - b[2] * c[1])
            + a[1] * (b[2] * c[0] - b[0] * c[2])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
        faces.push(p);
    }
    (faces, six / 6.)
}

/// Add the original rectangle's two directed lengths, with the 60 mm references reversed.
pub(crate) fn persist_rectangle_lengths(path: &std::path::Path) -> crate::sketch_facts::Source {
    use ferritecad_document::{
        AddLineConstraint, AddSketchConstraint, Document, LineConstraintKind, LineLengthMm,
        SketchConstraintEdits, SketchConstraintRule,
    };
    let mut document = Document::open(path).expect("doc");
    let (id, saved) = ferritecad_fixtures::sketch(&document).expect("rectangle");
    let curves = &saved.curves;
    let prepared = ferritecad_fixtures::constraint_changes(
        &document,
        id,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![
                AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve: curves[0].id,
                    kind: LineConstraintKind::Horizontal,
                }),
                AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve: curves[1].id,
                    kind: LineConstraintKind::Vertical,
                }),
                AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve: curves[0].id,
                    kind: LineConstraintKind::Distance(LineLengthMm::new(60.).expect("60")),
                }),
                AddSketchConstraint::Line(AddLineConstraint::Line {
                    curve: curves[1].id,
                    kind: LineConstraintKind::Distance(LineLengthMm::new(30.).expect("30")),
                }),
            ],
        },
    )
    .expect("prepare");
    document.write_changes(&prepared).expect("write lengths");
    let mut object = document.object(id).expect("read").expect("sketch");
    if let ferritecad_document::ObjectPayload::Sketch(sketch) = &mut object.payload {
        for c in &mut sketch.constraints {
            if let SketchConstraintRule::Distance { a, b, distance } = &mut c.rule
                && *distance
                    == ferritecad_document::Expression::constant(60.).expect("finite constant")
            {
                std::mem::swap(a, b);
            }
        }
    }
    document
        .write(|w| {
            w.put_object(
                object.id,
                object.parent,
                object.ordinal,
                object.name.as_deref(),
                &object.payload,
            )
        })
        .expect("reversed Distance endpoints remain selectable");
    let source = crate::sketch_facts::Source::read(&document).expect("catalog");
    document.close().expect("close");
    source
}

/// A stored (4.5, 0), R1.2 circle constrained to the origin and R10.
pub(crate) fn write_guessed_circle(path: &std::path::Path) {
    use ferritecad_document::{
        CircleConstraintKind, CircleRadiusMm, Document, SketchConstraintEdits, SketchCoordinateMm,
    };
    use ferritecad_kernel::OperationContext;

    ferritecad_jobs::create_document_with_kernel(
        ferritecad_jobs::CreateDocumentRequest::new(
            path,
            ferritecad_jobs::NewDocument::CircleExtrude(
                ferritecad_document::CircleExtrusion::new([4.5, 0.], 1.2, 15.).expect("a circle"),
            ),
            "test",
        ),
        ferritecad_occt::OcctKernel::new,
        &OperationContext::default(),
    )
    .expect("the source");
    let mut document = Document::open(path).expect("document");
    let (sketch, drawing) = ferritecad_fixtures::sketch(&document).expect("circle sketch");
    let circle = drawing.curves[0].id;
    let prepared = ferritecad_fixtures::constraint_changes(
        &document,
        sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: vec![
                AddSketchConstraint::Circle {
                    curve: circle,
                    kind: CircleConstraintKind::Radius(CircleRadiusMm::new(10.).expect("radius")),
                },
                AddSketchConstraint::Circle {
                    curve: circle,
                    kind: CircleConstraintKind::FixedCenter {
                        x: SketchCoordinateMm::new(0.).expect("x"),
                        y: SketchCoordinateMm::new(0.).expect("y"),
                    },
                },
            ],
        },
    )
    .expect("prepare");
    document.write_changes(&prepared).expect("rules");
    document.close().expect("close");
}

fn write_ruled_quadrilateral(
    path: &std::path::Path,
    rules: impl FnOnce(&[StableEntityId]) -> Vec<AddSketchConstraint>,
) {
    use ferritecad_document::{Document, SketchConstraintEdits};
    use ferritecad_kernel::OperationContext;

    ferritecad_jobs::create_document_with_kernel(
        ferritecad_jobs::CreateDocumentRequest::new(
            path,
            ferritecad_jobs::NewDocument::SketchExtrude(
                ferritecad_document::PolygonExtrusion::new(
                    vec![[-6., -3.], [6., -2.], [7., 5.], [-5., 4.]],
                    10.,
                )
                .expect("a guess"),
            ),
            "test",
        ),
        ferritecad_occt::OcctKernel::new,
        &OperationContext::default(),
    )
    .expect("the source");
    let mut document = Document::open(path).expect("document");
    let (sketch, drawing) = ferritecad_fixtures::sketch(&document).expect("profile sketch");
    let ids: Vec<_> = drawing.curves.iter().map(|c| c.id).collect();
    let prepared = ferritecad_fixtures::constraint_changes(
        &document,
        sketch,
        &SketchConstraintEdits {
            remove: vec![],
            add: rules(&ids),
        },
    )
    .expect("prepare");
    document.write_changes(&prepared).expect("rules");
    document.close().expect("close");
}

fn line_rule(curve: StableEntityId, kind: LineConstraintKind) -> AddSketchConstraint {
    AddSketchConstraint::Line(AddLineConstraint::Line { curve, kind })
}

fn side_length(curve: StableEntityId, value: f64) -> AddSketchConstraint {
    let mm = ferritecad_document::LineLengthMm::new(value).expect("side length");
    line_rule(curve, LineConstraintKind::Distance(mm))
}

fn fixed_origin(curve: StableEntityId) -> AddSketchConstraint {
    let origin = ferritecad_document::SketchCoordinateMm::new(0.).expect("origin");
    line_rule(
        curve,
        LineConstraintKind::Fixed {
            at: ferritecad_document::LineEndpoint::Start,
            x: origin,
            y: origin,
        },
    )
}

/// A rectangle whose Sketch STORES a rough guess (a 12 × 8 mm parallelogram about the origin) and
/// keeps the rules that solve it into a different shape: Line 1's start fixed at the origin, Line 1
/// horizontal and 60 mm, Line 2 vertical and 30 mm, and Lines 1 and 3 of **equal length** and
/// **parallel** (the EqualLength stored before the Parallel, so the two markers stand on one place
/// and a click reaches the EqualLength). Solved: (0,0) (60,0) (60,30) (0,30), no degree of freedom
/// left. Remove the EqualLength and the solver has one: the free corner stays where the Sketch
/// stored it, (-5, 4) slid onto the line y = 30 of the Parallel, a 65 mm Line 3.
pub(crate) fn write_marked_rectangle(path: &std::path::Path) {
    write_ruled_quadrilateral(path, |ids| {
        vec![
            fixed_origin(ids[0]),
            line_rule(ids[0], LineConstraintKind::Horizontal),
            side_length(ids[0], 60.),
            line_rule(ids[1], LineConstraintKind::Vertical),
            side_length(ids[1], 30.),
            AddSketchConstraint::Line(AddLineConstraint::EqualLength {
                a: ids[0],
                b: ids[2],
            }),
            AddSketchConstraint::Line(AddLineConstraint::Relation {
                a: ids[0],
                b: ids[2],
                relation: ferritecad_document::LineRelation::Parallel,
            }),
        ]
    });
}

/// A rectangle whose Sketch STORES a rough guess (a 12 × 8 mm parallelogram about the origin)
/// and keeps the rules that solve it into a 60 × 30 mm rectangle around the same place:
/// Lines 1 and 3 horizontal, Lines 2 and 4 vertical, Line 1 60 mm and Line 2 30 mm. Every
/// stored vertex and Line middle then lies inside the solved drawing, far from all of it.
pub(crate) fn write_dimensioned_rectangle(path: &std::path::Path) {
    write_ruled_quadrilateral(path, |ids| {
        vec![
            line_rule(ids[0], LineConstraintKind::Horizontal),
            line_rule(ids[1], LineConstraintKind::Vertical),
            line_rule(ids[2], LineConstraintKind::Horizontal),
            line_rule(ids[3], LineConstraintKind::Vertical),
            side_length(ids[0], 60.),
            side_length(ids[1], 30.),
        ]
    });
}

/// A quadrilateral whose Sketch STORES a rough guess (a 12 × 8 mm parallelogram about the origin)
/// and keeps the rules that solve it into a different shape: Line 1's start fixed at the origin,
/// Line 1 horizontal and 50 mm, Line 2 30 mm, Line 3 40 mm, and two saved directed angles — R_a
/// Line 1 start>end to Line 2 start>end, **+100°**, and R_b Line 2 **end>start** (the reversed
/// vector) to Line 3 start>end, **−100°**. Solved: (0,0) (50,0) (44.7906, 29.5442) (4.7906, 29.5442);
/// the stored guess turns ±77.1°, so no saved number is what the guess measures.
pub(crate) fn write_angled_quad(path: &std::path::Path) {
    use ferritecad_document::{DirectedAngle, DirectedLine, LineEndpoint};
    write_ruled_quadrilateral(path, |ids| {
        let side = |i: usize, start: LineEndpoint| {
            DirectedLine::new(ids[i], start, start.other()).expect("directed line")
        };
        let angle = |a, b, degrees| {
            AddSketchConstraint::Line(AddLineConstraint::DirectedAngle {
                a,
                b,
                angle: DirectedAngle::from_degrees(degrees).expect("angle"),
            })
        };
        vec![
            fixed_origin(ids[0]),
            line_rule(ids[0], LineConstraintKind::Horizontal),
            side_length(ids[0], 50.),
            angle(
                side(0, LineEndpoint::Start),
                side(1, LineEndpoint::Start),
                100.,
            ),
            side_length(ids[1], 30.),
            angle(
                side(1, LineEndpoint::End),
                side(2, LineEndpoint::Start),
                -100.,
            ),
            side_length(ids[2], 40.),
        ]
    });
}

/// The standalone SketchY Revolve frame, written without a kernel.
pub(crate) fn write_sector(path: &std::path::Path, points: &[[f64; 2]], degrees: f64) -> ObjectId {
    use ferritecad_document::{
        Body, DatumPlane, Dependency, DependencyRole, Document, Point2, Revolve, RevolveAxis,
        RevolveExtent, Sketch, SketchCurve, SolidOperation,
    };
    use ferritecad_types::{Result, StableEntityId, Transform};
    let mut d = Document::create(path).expect("document");
    let [plane, sketch, revolve, body] = std::array::from_fn(|_| ObjectId::new());
    let n = points.len();
    d.write(|w| {
        w.put_object(
            plane,
            None,
            0,
            Some("XY"),
            &ObjectPayload::DatumPlane(DatumPlane::Fixed {
                placement: Transform::IDENTITY,
            }),
        )?;
        let labels: Vec<StableEntityId> = (0..n).map(|_| StableEntityId::new()).collect();
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
                regions: Vec::new(),
                profile: sketch,
                axis: RevolveAxis::SketchY,
                extent: RevolveExtent::Partial {
                    angle: ferritecad_document::Expression::constant(f64::to_radians(degrees))?,
                },
                operation: SolidOperation::NewBody,
                previous: None,
            }),
        )?;
        w.put_object(
            body,
            None,
            3,
            Some("Body"),
            &ObjectPayload::Body(Body {
                tip_feature: Some(revolve),
                rollback: None,
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
