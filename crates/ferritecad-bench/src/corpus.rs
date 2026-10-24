// SPDX-License-Identifier: MIT
use std::path::{Path, PathBuf};

use ferritecad_document::*;
use ferritecad_types::{ObjectId, Result};

#[derive(Debug)]
pub struct Case {
    pub name: String,
    pub path: PathBuf,
    pub bodies: usize,
    pub body: ObjectId,
    pub extrude: Option<ObjectId>,
    pub fillet: Option<ObjectId>,
    pub revolve: Option<ObjectId>,
    pub additions: Vec<(&'static str, Vec<EditChange>)>,
    pub vertex: Option<(ObjectId, Sketch, Vec<SketchVertex>)>,
}

/// Every run writes fresh documents, with independent identities for each body.
pub fn generate(root: &Path, smoke: bool) -> Result<Vec<Case>> {
    let sizes = if smoke {
        vec![("plate", 1, 0)]
    } else {
        vec![
            ("plate", 1, 0),
            ("plate_16_cuts", 1, 16),
            ("10_bodies", 10, 4),
            ("50_bodies", 50, 4),
        ]
    };
    let mut cases = Vec::new();
    for (name, bodies, cuts) in sizes {
        let path = root.join(format!("{name}.fcad"));
        let mut output = Document::create(&path)?;
        for index in 0..bodies {
            let seed = root.join(format!("seed-{index}.fcad"));
            ferritecad_fixtures::write_plate(&seed)?;
            let mut document = Document::open(&seed)?;
            let body = body_of(&document)?;
            for cut in 0..cuts {
                let tool = ferritecad_fixtures::feature_changes::circular_tool(
                    &document,
                    body,
                    [6.0 + (cut % 4) as f64 * 12.0, 5.0 + (cut / 4) as f64 * 8.0],
                    1.0,
                    EndCondition::ThroughAll,
                )?;
                document.write_changes(&tool.changes)?;
            }
            let objects = document.objects()?;
            let dependencies = document.dependencies()?;
            let references = document.topology_refs()?;
            output.write(|writer| {
                for mut object in objects {
                    if let ObjectPayload::Sketch(sketch) = &mut object.payload {
                        let dx = (index % 10) as f64 * 70.0;
                        let dy = (index / 10) as f64 * 50.0;
                        let offset = |p: &mut Point2| {
                            p.x += dx;
                            p.y += dy;
                        };
                        for curve in &mut sketch.curves {
                            match &mut curve.geometry {
                                SketchGeometry::Line { start, end } => {
                                    offset(start);
                                    offset(end);
                                }
                                SketchGeometry::Circle { center, .. } => offset(center),
                                _ => unreachable!("corpus uses lines and circles"),
                            }
                        }
                    }
                    writer.put_object(
                        object.id,
                        object.parent,
                        index as i64 * 100 + object.ordinal,
                        object.name.as_deref(),
                        &object.payload,
                    )?;
                }
                for dependency in dependencies {
                    writer.add_dependency(dependency)?;
                }
                for reference in references {
                    writer.put_topology_ref(&reference)?;
                }
                Ok(())
            })?;
            document.close()?;
            std::fs::remove_file(&seed)
                .map_err(|e| ferritecad_types::CadError::io("removing corpus seed", e))?;
        }
        output.close()?;
        cases.push(describe(name, path, bodies)?);
    }
    if !smoke {
        let path = root.join("plate_4_fillets_constraints.fcad");
        let chain = ferritecad_fixtures::write_constrained_fillet_chain(&path, 2.0, 60.0, 40.0)?;
        let mut document = Document::open(&path)?;
        for corner in [1, 3] {
            let edge =
                ferritecad_fixtures::feature_changes::swept_edge(&document, chain.body, corner)?;
            document.write_changes(&[EditChange::AddFeature {
                object: ObjectId::new(),
                body: chain.body,
                name: Some("Fillet".into()),
                feature: FeatureAddition::Fillet {
                    edge,
                    radius_mm: Expression::constant(2.0)?,
                },
            }])?;
        }
        document.close()?;
        cases.push(describe("plate_4_fillets_constraints", path, 1)?);
        // Start from the fixture's writer graph; replace its profile and feature.
        let path = root.join("revolve.fcad");
        ferritecad_fixtures::write_plate(&path)?;
        let mut document = Document::open(&path)?;
        let objects = document.objects()?;
        let sketch_id = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("sketch")
            .id;
        document.write(|writer| {
            for object in objects {
                let payload = match object.payload {
                    ObjectPayload::Sketch(mut sketch) => {
                        let points = [[5.0, 0.0], [15.0, 0.0], [15.0, 20.0], [5.0, 20.0]];
                        for (i, curve) in sketch.curves.iter_mut().enumerate() {
                            curve.geometry = SketchGeometry::Line {
                                start: Point2::new(points[i][0], points[i][1])?,
                                end: Point2::new(points[(i + 1) % 4][0], points[(i + 1) % 4][1])?,
                            };
                        }
                        ObjectPayload::Sketch(sketch)
                    }
                    ObjectPayload::Extrude(_) => ObjectPayload::Revolve(Revolve {
                        profile: sketch_id,
                        axis: RevolveAxis::SketchY,
                        extent: RevolveExtent::Partial {
                            angle: ferritecad_document::Expression::constant(f64::to_radians(
                                180.0,
                            ))?,
                        },
                        operation: SolidOperation::NewBody,
                        axis_segment: None,
                    }),
                    payload => payload,
                };
                writer.put_object(
                    object.id,
                    object.parent,
                    object.ordinal,
                    object.name.as_deref(),
                    &payload,
                )?;
            }
            Ok(())
        })?;
        // Extrusion names are irrelevant to the turned fixture; write names for its faces.
        let refs = document.topology_refs()?;
        document.write(|writer| {
            for mut reference in refs {
                reference.output_role = match reference.output_role {
                    SemanticRole::ExtrudeCap { side } => SemanticRole::RevolveCap { side },
                    SemanticRole::ExtrudeSide { profile_segment } => {
                        SemanticRole::RevolveFace { profile_segment }
                    }
                    _ => unreachable!("plate references"),
                };
                writer.put_topology_ref(&reference)?;
            }
            Ok(())
        })?;
        document.close()?;
        cases.push(describe("revolve", path, 1)?);
    }
    Ok(cases)
}
fn body_of(document: &Document) -> Result<ObjectId> {
    Ok(document
        .objects()?
        .into_iter()
        .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
        .expect("fixture body")
        .id)
}
fn describe(name: &str, path: PathBuf, bodies: usize) -> Result<Case> {
    let document = Document::open_read_only(&path)?;
    let objects = document.objects()?;
    let body = body_of(&document)?;
    let find = |predicate: fn(&ObjectPayload) -> bool| {
        objects.iter().find(|o| predicate(&o.payload)).map(|o| o.id)
    };
    let extrude = document
        .expression_slots()?
        .into_iter()
        .find(|slot| {
            slot.slot == ferritecad_document::ExpressionSlot::ExtrudeDistance
                && slot.refusal.is_none()
        })
        .map(|slot| slot.object);
    let additions = if name == "plate" {
        vec![
            (
                "apply_add_cut",
                ferritecad_fixtures::feature_changes::circular_tool(
                    &document,
                    body,
                    [20., 20.],
                    2.,
                    EndCondition::ThroughAll,
                )?
                .changes,
            ),
            (
                "apply_add_fillet",
                vec![ferritecad_fixtures::feature_changes::edge_addition(
                    &document, body, 0, 2., false,
                )?],
            ),
        ]
    } else {
        Vec::new()
    };
    Ok(Case {
        additions,
        vertex: if name == "plate" {
            objects.iter().find_map(|o| {
                let ObjectPayload::Sketch(sketch) = &o.payload else {
                    return None;
                };
                let mut vertices: Vec<_> = sketch
                    .curves
                    .iter()
                    .filter_map(|c| match c.geometry {
                        SketchGeometry::Line { start, .. } => Some(SketchVertex {
                            curve_id: c.id,
                            start_mm: [start.x, start.y],
                        }),
                        _ => None,
                    })
                    .collect();
                vertices[0].start_mm[0] += 1.0;
                Some((o.id, sketch.clone(), vertices))
            })
        } else {
            None
        },
        name: name.to_owned(),
        path,
        bodies,
        body,
        extrude,
        fillet: find(|p| matches!(p, ObjectPayload::Fillet(_))),
        revolve: find(|p| matches!(p, ObjectPayload::Revolve(_))),
    })
}
