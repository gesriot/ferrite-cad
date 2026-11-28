// SPDX-License-Identifier: MIT
//! Read-only projections used by widget and independent geometry assertions.
#![allow(clippy::panic)]
use crate::feature_facts::{CircularCut, CutExtent, EdgeChamfer, EdgeFillet};
use ferritecad_document::{
    EditChange, EndCondition, FeatureAddition, ObjectPayload, SketchGeometry,
};
use ferritecad_jobs::EditRequest;
use ferritecad_types::ObjectId;

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
