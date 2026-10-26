// SPDX-License-Identifier: MIT
#![allow(dead_code, reason = "shared by focused naming test targets")]
use ferritecad_document::*;
use ferritecad_kernel::*;
use ferritecad_topology::ModelNaming;
use ferritecad_types::{ObjectId, Result, StableEntityId};
use std::collections::{BTreeMap, BTreeSet};

pub fn prepared(feature: ObjectId, profile: &Profile, revolve: bool) -> Result<ModelNaming> {
    let mut document = Document::create(":memory:")?;
    let plane = "00000000-0000-7000-8000-000000000001".parse::<ObjectId>()?;
    let sketch = "00000000-0000-7000-8000-000000000002".parse::<ObjectId>()?;
    let point = |p: PlanarPoint| Point2::new(p.x, p.y);
    let curves = profile
        .segments()
        .map(|s| {
            Ok(SketchCurve {
                id: s.label,
                construction: false,
                geometry: match s.geometry {
                    SegmentGeometry::Line { start, end } => SketchGeometry::Line {
                        start: point(start)?,
                        end: point(end)?,
                    },
                    SegmentGeometry::Circle { center, radius } => SketchGeometry::Circle {
                        center: point(center)?,
                        radius,
                    },
                    SegmentGeometry::Arc {
                        center,
                        radius,
                        start_angle,
                        end_angle,
                    } => SketchGeometry::Arc {
                        center: point(center)?,
                        radius,
                        start_angle,
                        end_angle,
                    },
                    _ => {
                        return Err(ferritecad_types::CadError::unsupported(
                            "unknown test curve",
                        ));
                    }
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;
    document.write(|w| {
        w.put_object(
            sketch,
            None,
            0,
            None,
            &ObjectPayload::Sketch(Sketch {
                plane,
                curves,
                constraints: vec![],
            }),
        )?;
        let payload = if revolve {
            ObjectPayload::Revolve(Revolve {
                profile: sketch,
                axis: ferritecad_document::RevolveAxis::SketchY,
                extent: RevolveExtent::FullTurn,
                operation: SolidOperation::NewBody,
                previous: None,
            })
        } else {
            ObjectPayload::Extrude(Extrude {
                profile: sketch,
                end_condition: EndCondition::Blind {
                    distance: Expression::constant(1.)?,
                },
                reversed: false,
                operation: SolidOperation::NewBody,
                previous: None,
            })
        };
        w.put_object(feature, None, 1, None, &payload)?;
        Ok(())
    })?;
    let mut model = ModelNaming::new();
    model.prepare(&document.objects()?.into_iter().map(|o| (o.id, o)).collect())?;
    Ok(model)
}
pub fn bind(
    kernel: &mut impl GeometryKernel,
    feature: ObjectId,
    profile: &Profile,
    result: &OperationResult,
) -> Result<ModelNaming> {
    let revolve = kernel.naming_history(result.shape)?.operation == Some(NamingOperation::Revolve);
    let mut model = prepared(feature, profile, revolve)?;
    let names = model.name_operation(feature, result.shape, &[], kernel)?;
    model.insert_feature(feature, names)?;
    Ok(model)
}
pub fn outputs(
    history: &NamingHistory,
    input: NamingInput,
    relation: NamingRelation,
    kind: SubShapeKind,
) -> Vec<SubShapeHandle> {
    history
        .relations
        .iter()
        .filter_map(|(i, r, o)| (*i == input && *r == relation).then_some(*o).flatten())
        .filter(|s| s.kind() == kind)
        .collect()
}
pub fn generated(
    kernel: &mut impl GeometryKernel,
    shape: ShapeHandle,
    input: NamingInput,
    kind: SubShapeKind,
) -> Vec<SubShapeHandle> {
    outputs(
        &kernel.naming_history(shape).expect("live provenance"),
        input,
        NamingRelation::Generated,
        kind,
    )
}
pub fn cap(kernel: &mut impl GeometryKernel, shape: ShapeHandle, end: bool) -> SubShapeHandle {
    let faces = generated(kernel, shape, NamingInput::Cap { end }, SubShapeKind::Face);
    assert_eq!(faces.len(), 1);
    faces[0]
}
pub fn signature(
    named: &ferritecad_topology::generative::FeatureNaming,
) -> BTreeMap<ferritecad_topology::generative::Name, BTreeSet<ferritecad_topology::generative::Name>>
{
    named
        .ancestry
        .iter()
        .map(|(name, members)| (*name, members.iter().map(|s| named.entities[s]).collect()))
        .collect()
}
pub fn sweep_edges(
    kernel: &mut impl GeometryKernel,
    shape: ShapeHandle,
) -> BTreeMap<ferritecad_types::ProfileJoint, SubShapeHandle> {
    let trace = kernel.naming_history(shape).expect("history");
    let faces: BTreeMap<_, _> = trace
        .relations
        .iter()
        .filter_map(|(i, r, o)| match (i, r, o) {
            (NamingInput::Curve(c), NamingRelation::Generated, Some(s))
                if s.kind() == SubShapeKind::Face =>
            {
                Some((*s, *c))
            }
            _ => None,
        })
        .collect();
    let topology = kernel.shape_topology(shape).expect("topology");
    let mut joints = BTreeMap::<_, Vec<_>>::new();
    for edge in topology.entities(SubShapeKind::Edge) {
        let adjacent: Vec<_> = topology.adjacent_faces[&edge]
            .iter()
            .filter_map(|f| faces.get(f))
            .collect();
        if adjacent.len() == 2 {
            let joint = ferritecad_types::ProfileJoint::new(*adjacent[0], *adjacent[1])
                .expect("two curves");
            joints.entry(joint).or_default().push(edge);
        }
    }
    joints
        .into_iter()
        .filter(|(_, v)| v.len() == 1)
        .map(|(j, v)| (j, v[0]))
        .collect()
}
pub fn faces(
    kernel: &mut impl GeometryKernel,
    result: &OperationResult,
    labels: &[StableEntityId],
) -> Vec<SubShapeHandle> {
    let mut faces: Vec<_> = labels
        .iter()
        .flat_map(|c| {
            generated(
                kernel,
                result.shape,
                NamingInput::Curve(*c),
                SubShapeKind::Face,
            )
        })
        .collect();
    faces.extend(generated(
        kernel,
        result.shape,
        NamingInput::Cap { end: false },
        SubShapeKind::Face,
    ));
    faces.extend(generated(
        kernel,
        result.shape,
        NamingInput::Cap { end: true },
        SubShapeKind::Face,
    ));
    faces
}
pub fn survival(trace: &NamingHistory, input: SubShapeHandle) -> NamingRelation {
    let found: BTreeSet<_> = trace
        .relations
        .iter()
        .filter(|(i, r, _)| *i == NamingInput::SubShape(input) && *r != NamingRelation::Generated)
        .map(|(_, r, _)| *r)
        .collect();
    assert_eq!(found.len(), 1, "one survival account");
    *found.first().expect("account")
}
pub fn surviving(
    trace: &NamingHistory,
    input: SubShapeHandle,
) -> std::vec::IntoIter<SubShapeHandle> {
    trace
        .relations
        .iter()
        .filter_map(|(i, r, o)| {
            (*i == NamingInput::SubShape(input)
                && matches!(r, NamingRelation::Kept | NamingRelation::Modified))
            .then_some(*o)
            .flatten()
        })
        .collect::<Vec<_>>()
        .into_iter()
}

pub fn cap_vertices(
    kernel: &mut impl GeometryKernel,
    shape: ShapeHandle,
    end: bool,
) -> BTreeMap<ferritecad_types::ProfileJoint, SubShapeHandle> {
    let cap = cap(kernel, shape, end);
    let swept = sweep_edges(kernel, shape);
    let topology = kernel.shape_topology(shape).expect("topology");
    swept
        .into_iter()
        .filter_map(|(joint, edge)| {
            let mut faces = topology.adjacent_faces[&edge].clone();
            faces.insert(cap);
            let vertices: Vec<_> = topology
                .entities(SubShapeKind::Vertex)
                .filter(|v| topology.adjacent_faces[v] == faces)
                .collect();
            (vertices.len() == 1).then(|| (joint, vertices[0]))
        })
        .collect()
}
pub fn cap_edges(
    kernel: &mut impl GeometryKernel,
    shape: ShapeHandle,
    end: bool,
) -> BTreeMap<StableEntityId, SubShapeHandle> {
    let cap = cap(kernel, shape, end);
    let trace = kernel.naming_history(shape).expect("history");
    let topology = kernel.shape_topology(shape).expect("topology");
    trace
        .relations
        .iter()
        .filter_map(|(i, r, o)| match (i, r, o) {
            (NamingInput::Curve(curve), NamingRelation::Generated, Some(face))
                if face.kind() == SubShapeKind::Face =>
            {
                let wanted = BTreeSet::from([*face, cap]);
                let edges: Vec<_> = topology
                    .entities(SubShapeKind::Edge)
                    .filter(|e| topology.adjacent_faces[e] == wanted)
                    .collect();
                (edges.len() == 1).then(|| (*curve, edges[0]))
            }
            _ => None,
        })
        .collect()
}

pub fn side_name(
    feature: ObjectId,
    plane: ObjectId,
    curve: StableEntityId,
) -> ferritecad_topology::generative::Name {
    use ferritecad_topology::generative::{curve_root, generated};
    generated(
        feature,
        NamingOperation::Extrude,
        curve_root(feature, curve, Some(plane)),
        SubShapeKind::Face,
    )
}
pub fn cap_name(
    feature: ObjectId,
    plane: ObjectId,
    end: bool,
) -> ferritecad_topology::generative::Name {
    use ferritecad_topology::generative::{cap_root, generated};
    generated(
        feature,
        NamingOperation::Extrude,
        cap_root(feature, end, Some(plane)),
        SubShapeKind::Face,
    )
}
pub fn resolve(
    model: &ModelNaming,
    feature: ObjectId,
    name: &ferritecad_topology::generative::Name,
) -> Result<Vec<SubShapeHandle>> {
    let feature = model
        .feature(feature)
        .ok_or_else(|| ferritecad_types::CadError::topology("feature has no names"))?;
    Ok(feature
        .resolve_set(*name, ferritecad_topology::generative::Selection::One)?
        .iter()
        .copied()
        .collect())
}

pub fn plane_id() -> ObjectId {
    "00000000-0000-7000-8000-000000000001"
        .parse()
        .expect("test plane UUID")
}

// Select the test specimen by named face incidence; retain its primary name
// across edits or archives, and check the resolved entity against the mesh.
pub fn name_at_incidence(
    model: &ModelNaming,
    feature: ObjectId,
    kind: SubShapeKind,
    faces: impl IntoIterator<Item = ferritecad_topology::generative::Name>,
) -> Result<ferritecad_topology::generative::Name> {
    let named = model
        .feature(feature)
        .ok_or_else(|| ferritecad_types::CadError::topology("feature has no names"))?;
    let faces = faces
        .into_iter()
        .map(|name| named.resolve(name))
        .collect::<Result<BTreeSet<_>>>()?;
    let entities: Vec<_> = named
        .topology
        .entities(kind)
        .filter(|entity| named.topology.adjacent_faces[entity] == faces)
        .collect();
    if entities.len() != 1 {
        return Err(ferritecad_types::CadError::topology(format!(
            "test incidence selects {} entities",
            entities.len()
        )));
    }
    Ok(named.entities[&entities[0]])
}
