// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_kernel::SubShapeKind;
use ferritecad_types::{CanonicalHasher, Vec3};

fn id(value: u64) -> ObjectId {
    format!("00000000-0000-7000-8000-{value:012x}")
        .parse()
        .expect("fixed fixture identity")
}
fn save(d: &mut Document, value: u64, payload: ObjectPayload) -> ObjectId {
    let object = id(value);
    d.write(|w| {
        w.put_object(object, None, 0, None, &payload)?;
        for dep in payload.dependencies(object) {
            w.add_dependency(dep)?;
        }
        Ok(())
    })
    .expect("fixture object");
    object
}
fn profile(d: &mut Document, value: u64, plane: ObjectId, a: [f64; 2], b: [f64; 2]) -> ObjectId {
    let mut sketch = ferritecad_fixtures::result_sets::rectangle(plane, a, b).expect("rectangle");
    for (i, curve) in sketch.curves.iter_mut().enumerate() {
        curve.id = StableEntityId::from_bytes(id(100 + value * 4 + i as u64).to_bytes())
            .expect("fixed curve identity");
    }
    save(d, value, ObjectPayload::Sketch(sketch))
}
fn sweep(
    d: &mut Document,
    value: u64,
    profile: ObjectId,
    previous: Option<ObjectId>,
    operation: SolidOperation,
    end_condition: EndCondition,
    reversed: bool,
) -> ObjectId {
    save(
        d,
        value,
        ObjectPayload::Extrude(Extrude {
            profile,
            previous,
            operation,
            regions: vec![],
            reversed,
            end_condition,
        }),
    )
}

pub(super) fn append_measurements(facts: &mut String) {
    for behind in [false, true] {
        for reversed in [false, true] {
            for operation in [SolidOperation::Cut, SolidOperation::Intersect] {
                let root = tempfile::tempdir().expect("root");
                let mut d = Document::create(root.path().join("set.fcad")).expect("document");
                let plane = save(
                    &mut d,
                    1,
                    ObjectPayload::DatumPlane(DatumPlane::Fixed {
                        placement: Transform::IDENTITY,
                    }),
                );
                let first = profile(&mut d, 2, plane, [0., 0.], [60., 40.]);
                let first = sweep(
                    &mut d,
                    3,
                    first,
                    None,
                    SolidOperation::NewBody,
                    blind(10.),
                    reversed,
                );
                let second = profile(&mut d, 4, plane, [70., 0.], [80., 10.]);
                let mut previous = sweep(
                    &mut d,
                    5,
                    second,
                    Some(first),
                    SolidOperation::Add,
                    blind(25.),
                    reversed,
                );
                let sign = if reversed { -1. } else { 1. };
                if behind {
                    let plane = save(
                        &mut d,
                        6,
                        ObjectPayload::DatumPlane(DatumPlane::Fixed {
                            placement: Transform::from_translation(
                                Vec3::new(0., 0., -30. * sign).expect("behind"),
                            )
                            .expect("placement"),
                        }),
                    );
                    let back = profile(&mut d, 7, plane, [-20., 0.], [-10., 10.]);
                    previous = sweep(
                        &mut d,
                        8,
                        back,
                        Some(previous),
                        SolidOperation::Add,
                        blind(10.),
                        reversed,
                    );
                }
                let tool_plane = save(
                    &mut d,
                    9,
                    ObjectPayload::DatumPlane(DatumPlane::Fixed {
                        placement: Transform::from_translation(
                            Vec3::new(0., 0., -sign).expect("start"),
                        )
                        .expect("placement"),
                    }),
                );
                let mut circles =
                    ferritecad_fixtures::datum_planes::circle(tool_plane, [20., 20.], 2.)
                        .expect("first tool region");
                circles.curves.extend(
                    ferritecad_fixtures::datum_planes::circle(tool_plane, [75., 5.], 2.)
                        .expect("second tool region")
                        .curves,
                );
                for (i, curve) in circles.curves.iter_mut().enumerate() {
                    curve.id = StableEntityId::from_bytes(id(200 + i as u64).to_bytes())
                        .expect("fixed tool root");
                }
                let tool = save(&mut d, 10, ObjectPayload::Sketch(circles));
                let tip = sweep(
                    &mut d,
                    11,
                    tool,
                    Some(previous),
                    operation,
                    EndCondition::ThroughAll,
                    reversed,
                );
                // Another Body is deliberately taller and never participates in the bounds.
                let unrelated = profile(&mut d, 12, plane, [200., 0.], [210., 10.]);
                sweep(
                    &mut d,
                    13,
                    unrelated,
                    None,
                    SolidOperation::NewBody,
                    blind(100.),
                    reversed,
                );
                let removed = 140. * std::f64::consts::PI;
                let cut = operation == SolidOperation::Cut;
                let expected = if cut {
                    26500. + if behind { 1000. } else { 0. } - removed
                } else {
                    removed
                };
                check(&d, tip, expected);
                let mut k = OcctKernel::new().expect("kernel");
                let built =
                    rebuild_cold(&d, &mut k, &OperationContext::default()).expect("rebuild");
                let shape = built.shape(tip).expect("result");
                k.validate_solids(shape)
                    .expect("each member valid and positive");
                let count = k.solid_count(shape).expect("solid count");
                assert_eq!(count, if cut && behind { 3 } else { 2 });
                let bounds = k
                    .projected_bounds(
                        built.shape(previous).expect("whole target"),
                        Vec3::new(0., 0., sign).expect("direction"),
                    )
                    .expect("bounds");
                assert_eq!(bounds, [if behind { -30. } else { 0. }, 25.]);
                let ObjectPayload::Extrude(e) =
                    d.object(tip).expect("object").expect("tip").payload
                else {
                    unreachable!("extrusion");
                };
                let request = ferritecad_eval::extrude_request(
                    &e,
                    built.profile(tool).expect("profile").clone(),
                    Some(bounds),
                )
                .expect("extent");
                assert_eq!(request.extent().total_length(), 27.);
                let names = &built.topology().features[&tip];
                let mut digest = CanonicalHasher::new("through-all-measured-names");
                for (name, entities) in &names.bindings {
                    digest.hash(&name.digest).u64(entities.len() as u64);
                }
                facts.push_str(&format!(
                    "{}\t{}\t{}\t{:.12}\t{:.12}\t{:.12}\t{:.12}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    if behind { "set-behind" } else { "set" },
                    reversed,
                    if cut { "Cut" } else { "Intersect" },
                    bounds[0],
                    bounds[1],
                    request.extent().total_length(),
                    k.shape_stats(shape).expect("volume").1,
                    names.topology.entities(SubShapeKind::Face).count(),
                    names.topology.entities(SubShapeKind::Edge).count(),
                    names.topology.entities(SubShapeKind::Vertex).count(),
                    names.ambiguous().count(),
                    count,
                    digest.finish(),
                ));
                built.release_all(&mut k);
                save(
                    &mut d,
                    9,
                    ObjectPayload::DatumPlane(DatumPlane::Fixed {
                        placement: Transform::from_translation(
                            Vec3::new(0., 0., 40. * sign).expect("past entire target"),
                        )
                        .expect("placement"),
                    }),
                );
                let failed =
                    rebuild_cold(&d, &mut k, &OperationContext::default()).expect("report");
                assert_eq!(failed.report().failed.len(), 1);
                assert!(failed.report().failed[0].message.contains("nothing ahead"));
                failed.release_all(&mut k);
            }
        }
    }
}
