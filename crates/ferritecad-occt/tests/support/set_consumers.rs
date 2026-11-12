// SPDX-License-Identifier: MIT
use super::*;
use ferritecad_kernel::{ChamferRequest, FilletRequest, NamingInput, NamingRelation};

fn operate(
    kernel: &mut OcctKernel,
    target: ferritecad_kernel::ShapeHandle,
    edges: &[ferritecad_kernel::SubShapeHandle],
    amount: f64,
    chamfer: bool,
) -> ferritecad_types::Result<ferritecad_kernel::OperationResult> {
    let context = OperationContext::default();
    if chamfer {
        kernel.chamfer_edges(&ChamferRequest::new(target, edges, amount)?, &context)
    } else {
        kernel.fillet_edges(&FilletRequest::new(target, edges, amount)?, &context)
    }
}

#[test]
fn one_failed_edge_refuses_the_whole_set_without_mutating_its_source() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let mut doc = Document::create(root.path().join("atomic.fcad")).expect("document");
    let p = plate(&mut doc).expect("plate");
    let edges = [0, 1].map(|corner| {
        ferritecad_fixtures::feature_changes::plate_edge(&doc, p.body, corner)
            .expect("named corner")
    });
    let mut kernel = OcctKernel::new().expect("kernel");
    let mut s = slot(&doc, &p, &mut kernel).expect("slot");
    let EditChange::AddSketch { sketch, .. } = &mut s.changes[0] else {
        unreachable!("slot sketch");
    };
    *sketch = rectangle(p.plane, [54., -2.], [58., 42.]).expect("narrow second piece");
    doc.write_changes(&s.changes).expect("split");
    let built = build(&doc, &mut kernel);
    let target = built.shape(s.keep).expect("complete target");
    let selected: Vec<_> = edges
        .iter()
        .map(|edge| {
            built
                .topology()
                .resolve(EntityKind::Edge, s.keep, s.keep, &[*edge])
                .expect("named edge")[0]
        })
        .collect();
    let bytes = kernel.encode_shape(target).expect("source bytes");
    let live = kernel.live_shape_count();
    for chamfer in [false, true] {
        let one = operate(&mut kernel, target, &selected[..1], 3., chamfer)
            .expect("the first piece permits this amount");
        assert!(kernel.is_valid(one.shape).expect("valid first piece edit"));
        kernel.release(one.shape);
        let error = operate(&mut kernel, target, &selected, 3., chamfer)
            .expect_err("second piece fails the whole feature");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Kernel);
        assert!(
            error
                .to_string()
                .contains(if chamfer { "chamfer" } else { "round" }),
            "{error}"
        );
        assert_eq!(
            kernel.live_shape_count(),
            live,
            "no partial result published"
        );
        assert_eq!(
            kernel.encode_shape(target).expect("unchanged target"),
            bytes
        );
    }
    built.release_all(&mut kernel);
}

#[test]
fn an_edge_shared_by_two_native_solids_refuses_without_an_arbitrary_owner() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let mut doc = Document::create(root.path().join("shared.fcad")).expect("document");
    let p = plate(&mut doc).expect("plate");
    let edge = ferritecad_fixtures::feature_changes::plate_edge(&doc, p.body, 2)
        .expect("corner at the contact");
    let (profile, addition) = (ObjectId::new(), ObjectId::new());
    doc.write_changes(&[
        EditChange::AddSketch {
            object: profile,
            name: None,
            sketch: rectangle(p.plane, [60., 40.], [70., 50.]).expect("contact at one edge"),
        },
        EditChange::AddFeature {
            object: addition,
            body: p.body,
            name: None,
            feature: extrusion(profile, SolidOperation::Add, 10.).expect("add"),
        },
    ])
    .expect("edge-touching solids");
    let mut kernel = OcctKernel::new().expect("kernel");
    let built = build(&doc, &mut kernel);
    measure(&built, addition, &mut kernel, 2, 25000.);
    let selected = built
        .topology()
        .resolve(EntityKind::Edge, addition, addition, &[edge])
        .expect("unique shared edge name");
    let target = built.shape(addition).expect("target");
    let bytes = kernel.encode_shape(target).expect("source bytes");
    let live = kernel.live_shape_count();
    for chamfer in [false, true] {
        let error =
            operate(&mut kernel, target, &selected, 0.5, chamfer).expect_err("two owning solids");
        assert_eq!(error.kind(), ferritecad_types::ErrorKind::Input);
        assert!(
            error
                .to_string()
                .contains("a selected edge belongs to 2 solids"),
            "{error}"
        );
        assert_eq!(kernel.live_shape_count(), live);
        assert_eq!(
            kernel.encode_shape(target).expect("source unchanged"),
            bytes
        );
    }
    built.release_all(&mut kernel);
}

pub(super) fn check_complete_history(
    built: &RebuildResult,
    previous: ObjectId,
    feature: ObjectId,
    kernel: &mut OcctKernel,
) {
    use std::collections::BTreeSet;
    let input = &built.topology().features[&previous];
    let output = &built.topology().features[&feature];
    let history = kernel
        .naming_history(built.shape(feature).expect("shape"))
        .expect("history");
    let accounted: BTreeSet<_> = history
        .relations
        .iter()
        .filter_map(|(i, _, _)| {
            if let NamingInput::SubShape(e) = i {
                Some(*e)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(accounted, input.entities.keys().copied().collect());
    let faces: BTreeSet<_> = history
        .relations
        .iter()
        .filter_map(|(_, _, o)| *o)
        .filter(|o| o.kind() == SubShapeKind::Face)
        .collect();
    assert_eq!(
        faces,
        output.topology.entities(SubShapeKind::Face).collect()
    );
    for (i, relation, o) in &history.relations {
        if let (NamingInput::SubShape(i), NamingRelation::Kept, Some(o)) = (i, relation, o) {
            assert_eq!(
                input.entities[i], output.entities[o],
                "Kept names remain exact"
            );
        }
    }
}

#[test]
fn planar_set_supports_multiple_regions_vertex_offsets_and_a_face_datum() {
    use ferritecad_fixtures::datum_planes::circle;
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let mut doc = ferritecad_fixtures::open_plate(root.path()).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let ceiling =
        ferritecad_fixtures::up_to_end::ceiling(&mut doc, &mut kernel).expect("ceiling and boss");
    let body = doc
        .objects()
        .expect("objects")
        .into_iter()
        .find_map(|o| {
            matches!(o.payload, ObjectPayload::Body(Body { tip_feature: Some(tip) , ..})
            if tip == ceiling.result)
            .then_some(o.id)
        })
        .expect("support body");
    let (extra_profile, support, datum, sketch, attachment) = (
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
    );
    doc.write_changes(&[
        EditChange::AddSketch {
            object: extra_profile,
            name: None,
            sketch: circle(ceiling.plane, [120., 20.], 10.).expect("detached ceiling"),
        },
        EditChange::AddFeature {
            object: support,
            body,
            name: None,
            feature: extrusion(extra_profile, SolidOperation::Add, 3.).expect("add"),
        },
        EditChange::AddDatumPlane {
            object: datum,
            name: None,
            plane: DatumPlane::Face {
                result: support,
                face: ceiling.face,
                offset_mm: Expression::constant(0.).expect("offset"),
            },
        },
        EditChange::AddSketch {
            object: sketch,
            name: None,
            sketch: circle(datum, [25., -20.], 1.).expect("attached sketch"),
        },
        EditChange::AddBody {
            object: ObjectId::new(),
            name: None,
            feature_object: attachment,
            feature_name: None,
            feature: extrusion(sketch, SolidOperation::NewBody, 2.).expect("attached extrusion"),
        },
    ])
    .expect("set and datum");
    let mut profile = circle(ceiling.boss.plane, [18., 20.], 1.5).expect("first region");
    profile.curves.extend(
        circle(ceiling.boss.plane, [32., 20.], 1.5)
            .expect("second region")
            .curves,
    );
    doc.write(|w| {
        w.put_object(
            ceiling.boss.sketch,
            None,
            0,
            None,
            &ObjectPayload::Sketch(profile),
        )
    })
    .expect("multiple regions");
    for (vertex, offset) in [(false, 0.), (false, 2.), (false, -2.), (true, 0.)] {
        let condition = if vertex {
            EndCondition::UpToVertex {
                result: support,
                vertex: ceiling.vertex,
            }
        } else {
            EndCondition::UpToFace {
                result: support,
                face: ceiling.face,
                offset_mm: Expression::constant(offset).expect("offset"),
            }
        };
        doc.write_changes(&[EditChange::SetEndCondition {
            object: ceiling.boss.feature,
            end_condition: condition,
        }])
        .expect("support in full set");
        let built = build(&doc, &mut kernel);
        measure(&built, support, &mut kernel, 2, 600. * std::f64::consts::PI);
        measure(
            &built,
            ceiling.boss.feature,
            &mut kernel,
            1,
            24000. + 4.5 * std::f64::consts::PI * (20. - offset),
        );
        measure(
            &built,
            attachment,
            &mut kernel,
            1,
            2. * std::f64::consts::PI,
        );
        let plane = built.plane(datum).expect("computed face plane");
        assert_eq!(plane.origin().z, 30.);
        assert_eq!(plane.normal().z, -1.);
        warm(&doc, &built, &mut kernel);
        built.release_all(&mut kernel);
    }
    doc.write_changes(&[
        EditChange::constant(ceiling.plane, ExpressionSlot::DatumOffset, 30.)
            .expect("move support"),
    ])
    .expect("changed support");
    let moved = build(&doc, &mut kernel);
    measure(
        &moved,
        ceiling.boss.feature,
        &mut kernel,
        1,
        24000. + 4.5 * std::f64::consts::PI * 30.,
    );
    assert_eq!(moved.plane(datum).expect("moving datum").origin().z, 40.);
    warm(&doc, &moved, &mut kernel);
    moved.release_all(&mut kernel);
    // Deleting the selected piece keeps a nonempty support result, but loses its references.
    let ObjectPayload::Sketch(extra) = doc
        .object(extra_profile)
        .expect("object")
        .expect("extra profile")
        .payload
    else {
        unreachable!("sketch");
    };
    let name = ferritecad_topology::generative::generated(
        support,
        ferritecad_kernel::NamingOperation::Extrude,
        ferritecad_topology::generative::curve_root(
            support,
            extra.curves[0].id,
            Some(ceiling.plane),
        ),
        SubShapeKind::Face,
    );
    let other = EntityReference::new(ReferenceMeaning {
        producer: support,
        kind: EntityKind::Face,
        digest: name.digest,
        selection: Selection::One,
    });
    let keep = ObjectId::new();
    doc.write_changes(&[EditChange::AddFeature {
        object: keep,
        body,
        name: None,
        feature: FeatureAddition::KeepSolids {
            selection: KeepSelection::ContainingFaces(vec![other]),
        },
    }])
    .expect("keep other support piece");
    for vertex in [false, true] {
        let reference = if vertex { ceiling.vertex } else { ceiling.face };
        let condition = if vertex {
            EndCondition::UpToVertex {
                result: keep,
                vertex: reference,
            }
        } else {
            EndCondition::UpToFace {
                result: keep,
                face: reference,
                offset_mm: Expression::constant(0.).expect("offset"),
            }
        };
        doc.write_changes(&[EditChange::SetEndCondition {
            object: ceiling.boss.feature,
            end_condition: condition,
        }])
        .expect("saved broken reference");
        let built = rebuild_cold(&doc, &mut kernel, &OperationContext::default())
            .expect("repairable document");
        assert_eq!(built.report().failed.len(), 1);
        let failure = &built.report().failed[0];
        assert_eq!(failure.id, ceiling.boss.feature);
        assert_eq!(failure.kind, "topology");
        assert!(failure.message.contains(if vertex {
            "vertex deleted"
        } else {
            "face deleted"
        }));
        assert!(failure.message.contains(&reference.id.to_string()));
        built.release_all(&mut kernel);
    }
}

#[test]
fn curved_face_in_a_set_keeps_offsets_full_coverage_and_complete_names() {
    use ferritecad_fixtures::datum_planes::{circle, sweep_face};
    use ferritecad_types::{Transform, Vec3};
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let mut doc = Document::create(root.path().join("curved.fcad")).expect("document");
    let (roof_plane, roof_profile, roof, base_plane, profile, feature) = (
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
    );
    let mut circles = circle(roof_plane, [0., 0.], 50.).expect("curved roof");
    let first_curve = circles.curves[0].id;
    circles.curves.extend(
        circle(roof_plane, [0., 200.], 50.)
            .expect("other member")
            .curves,
    );
    let placement = Transform::from_rotation(Vec3::Y, std::f64::consts::FRAC_PI_2)
        .expect("rotation")
        .then(
            &Transform::from_translation(Vec3::new(0., 20., 100.).expect("roof location"))
                .expect("translation"),
        )
        .expect("placement");
    doc.write_changes(&[
        EditChange::AddDatumPlane {
            object: roof_plane,
            name: None,
            plane: DatumPlane::Fixed { placement },
        },
        EditChange::AddSketch {
            object: roof_profile,
            name: None,
            sketch: circles,
        },
        EditChange::AddBody {
            object: ObjectId::new(),
            name: None,
            feature_object: roof,
            feature_name: None,
            feature: extrusion(roof_profile, SolidOperation::NewBody, 60.).expect("roof"),
        },
        EditChange::AddDatumPlane {
            object: base_plane,
            name: None,
            plane: DatumPlane::Fixed {
                placement: Transform::IDENTITY,
            },
        },
        EditChange::AddSketch {
            object: profile,
            name: None,
            sketch: rectangle(base_plane, [0., 0.], [60., 40.]).expect("profile"),
        },
    ])
    .expect("support set");
    let mut kernel = OcctKernel::new().expect("kernel");
    let built = build(&doc, &mut kernel);
    measure(&built, roof, &mut kernel, 2, 300000. * std::f64::consts::PI);
    let face = sweep_face(&built, &mut kernel, roof, NamingInput::Curve(first_curve))
        .expect("named curved face in one member");
    built.release_all(&mut kernel);
    doc.write_changes(&[EditChange::AddBody {
        object: ObjectId::new(),
        name: None,
        feature_object: feature,
        feature_name: None,
        feature: extrusion(profile, SolidOperation::NewBody, 50.).expect("initial blind"),
    }])
    .expect("consumer");
    for offset in [0., 3., -3.] {
        doc.write_changes(&[EditChange::SetEndCondition {
            object: feature,
            end_condition: EndCondition::UpToFace {
                result: roof,
                face,
                offset_mm: Expression::constant(offset).expect("offset"),
            },
        }])
        .expect("curved end");
        let built = build(&doc, &mut kernel);
        let radius: f64 = 50. + offset;
        let arc = 20. * (radius * radius - 400.).sqrt() + radius * radius * (20. / radius).asin();
        measure(&built, feature, &mut kernel, 1, 60. * (4000. - arc));
        let name = ferritecad_topology::generative::generated(
            feature,
            ferritecad_kernel::NamingOperation::Extrude,
            ferritecad_topology::generative::cap_root(feature, true, Some(base_plane)),
            SubShapeKind::Face,
        );
        let cap = EntityReference::new(ReferenceMeaning {
            producer: feature,
            kind: EntityKind::Face,
            digest: name.digest,
            selection: Selection::AllDescendants,
        });
        let caps = built
            .topology()
            .resolve(EntityKind::Face, feature, feature, &[cap])
            .expect("complete end cap family");
        assert_eq!(caps.len(), 2, "the cylindrical seam splits the ending cap");
        for cap in caps {
            assert_eq!(
                kernel.face_surface(cap).expect("native offset surface"),
                ferritecad_kernel::FaceSurface::Cylinder { radius }
            );
        }
        warm(&doc, &built, &mut kernel);
        built.release_all(&mut kernel);
    }
    // One uncompleted profile region refuses the whole feature, even with another valid region.
    let mut regions = rectangle(base_plane, [0., 0.], [60., 40.]).expect("covered region");
    regions.curves.extend(
        rectangle(base_plane, [0., 80.], [60., 90.])
            .expect("uncovered region")
            .curves,
    );
    doc.write(|w| w.put_object(profile, None, 0, None, &ObjectPayload::Sketch(regions)))
        .expect("two regions");
    let built = rebuild_cold(&doc, &mut kernel, &OperationContext::default()).expect("rebuild");
    let failure = &built.report().failed[0];
    assert_eq!(failure.id, feature);
    assert_eq!(failure.kind, "input");
    assert!(
        failure.message.contains("does not cover the profile"),
        "{}",
        failure.message
    );
    assert!(built.shape(feature).is_none());
    built.release_all(&mut kernel);
}

#[test]
fn a_named_selected_piece_merges_and_splits_without_retargeting_its_witness() {
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let mut doc = Document::create(root.path().join("merge-split.fcad")).expect("document");
    let p = plate(&mut doc).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let s = slot(&doc, &p, &mut kernel).expect("slot");
    doc.write_changes(&s.changes).expect("split");
    doc.write_changes(&[EditChange::SetKeepSelection {
        object: s.keep,
        selection: KeepSelection::ContainingFaces(vec![s.side]),
    }])
    .expect("select left piece by face");
    let (profile, merged, selected, cut_profile, cut, retained) = (
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
    );
    doc.write_changes(&[
        EditChange::AddSketch {
            object: profile,
            name: None,
            sketch: rectangle(p.plane, [20., 0.], [60., 40.]).expect("bridge material"),
        },
        EditChange::AddFeature {
            object: merged,
            body: p.body,
            name: None,
            feature: extrusion(profile, SolidOperation::Add, 10.).expect("merge"),
        },
        EditChange::AddFeature {
            object: selected,
            body: p.body,
            name: None,
            feature: FeatureAddition::KeepSolids {
                selection: KeepSelection::ContainingFaces(vec![s.side]),
            },
        },
        EditChange::AddSketch {
            object: cut_profile,
            name: None,
            sketch: rectangle(p.plane, [28., -2.], [32., 42.]).expect("new slot"),
        },
        EditChange::AddFeature {
            object: cut,
            body: p.body,
            name: None,
            feature: extrusion(cut_profile, SolidOperation::Cut, 11.).expect("split again"),
        },
        EditChange::AddFeature {
            object: retained,
            body: p.body,
            name: None,
            feature: FeatureAddition::KeepSolids {
                selection: KeepSelection::ContainingFaces(vec![s.side]),
            },
        },
    ])
    .expect("merged witness and second split");
    let built = build(&doc, &mut kernel);
    for (id, count, volume) in [
        (merged, 1, 24000.),
        (selected, 1, 24000.),
        (cut, 2, 22400.),
        (retained, 1, 11200.),
    ] {
        measure(&built, id, &mut kernel, count, volume);
        let face = built
            .topology()
            .resolve(EntityKind::Face, id, id, &[s.side])
            .expect("original surviving witness")[0];
        assert_eq!(
            built.topology().features[&id].entities[&face].digest,
            s.side.meaning.digest
        );
    }
    assert_eq!(signature(&built)[&merged], signature(&built)[&selected]);
    warm(&doc, &built, &mut kernel);
    built.release_all(&mut kernel);
    // A split parent remains ambiguous; keeping a piece never turns it into an unsplit parent.
    doc.write_changes(&[EditChange::SetKeepSelection {
        object: retained,
        selection: KeepSelection::ContainingFaces(vec![s.split]),
    }])
    .expect("saved ambiguous choice");
    let built = rebuild_cold(&doc, &mut kernel, &OperationContext::default()).expect("repair");
    let failure = &built.report().failed[0];
    assert_eq!(failure.id, retained);
    assert_eq!(failure.kind, "topology");
    assert!(
        failure.message.contains("split into 2 faces"),
        "{}",
        failure.message
    );
    built.release_all(&mut kernel);
}

pub(super) fn check_untouched_piece(
    built: &RebuildResult,
    previous: ObjectId,
    feature: ObjectId,
    witness: EntityReference,
    kernel: &mut OcctKernel,
) {
    let faces = built
        .topology()
        .resolve(EntityKind::Face, feature, previous, &[witness])
        .expect("untouched piece witness");
    let piece = kernel
        .keep_solids(
            built.shape(previous).expect("predecessor"),
            &[ferritecad_kernel::SolidWitness {
                reference: witness.id,
                faces,
            }],
            &OperationContext::default(),
        )
        .expect("isolate witness member")
        .shape;
    let selection = kernel.naming_history(piece).expect("selection history");
    let history = kernel
        .naming_history(built.shape(feature).expect("edge output"))
        .expect("edge history");
    let mut count = 0;
    for (input, relation, _) in selection.relations {
        if relation == NamingRelation::Kept {
            let NamingInput::SubShape(source) = input else {
                unreachable!("selection input");
            };
            let outputs: Vec<_> = history
                .relations
                .iter()
                .filter(|(i, _, _)| *i == input)
                .collect();
            assert_eq!(
                outputs.len(),
                1,
                "untouched elementary input has just Kept history"
            );
            let (_, relation, output) = outputs[0];
            assert_eq!(*relation, NamingRelation::Kept);
            assert_eq!(
                built.topology().features[&previous].entities[&source],
                built.topology().features[&feature].entities[&output.expect("kept output")]
            );
            count += 1;
        }
    }
    assert_eq!(
        count, 26,
        "every face, edge and vertex of the untouched piece"
    );
    kernel.release(piece);
}

#[test]
fn planar_family_across_pieces_requires_explicit_cardinality_and_one_outward_plane() {
    use ferritecad_fixtures::datum_planes::circle;
    use ferritecad_types::{Transform, Vec3};
    if !native() {
        return;
    }
    let root = tempfile::tempdir().expect("root");
    let mut doc = Document::create(root.path().join("family.fcad")).expect("document");
    let p = plate(&mut doc).expect("plate");
    let mut kernel = OcctKernel::new().expect("kernel");
    let s = slot(&doc, &p, &mut kernel).expect("slot");
    doc.write_changes(&s.changes).expect("split");
    let mut family = s.split;
    family.meaning.selection = Selection::AllDescendants;
    let built = build(&doc, &mut kernel);
    let faces = built
        .topology()
        .resolve(EntityKind::Face, s.keep, s.keep, &[family])
        .expect("two side descendants");
    assert_eq!(faces.len(), 2);
    assert!(
        built
            .topology()
            .resolve(EntityKind::Face, s.keep, s.keep, &[s.split])
            .expect_err("exact split")
            .to_string()
            .contains("split into 2 faces")
    );
    let plane = kernel
        .support_plane(&faces, &OperationContext::default())
        .expect("common plane");
    assert_eq!(plane.normal().y, -1.);
    let wrong = built
        .topology()
        .resolve(EntityKind::Face, s.keep, s.keep, &[s.side, s.other_side])
        .expect("opposite side references");
    let error = kernel
        .support_plane(&wrong, &OperationContext::default())
        .expect_err("opposite outward normals");
    assert!(
        error
            .to_string()
            .contains("one plane with one outward normal")
    );
    built.release_all(&mut kernel);
    let (datum, attached_profile, attachment, start, profile, feature) = (
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
        ObjectId::new(),
    );
    let placement = Transform::from_rotation(Vec3::X, -std::f64::consts::FRAC_PI_2)
        .expect("forward frame")
        .then(
            &Transform::from_translation(Vec3::new(0., -10., 0.).expect("start"))
                .expect("translation"),
        )
        .expect("placement");
    let mut regions = circle(start, [10., -5.], 1.).expect("first region");
    regions
        .curves
        .extend(circle(start, [50., -5.], 1.).expect("second region").curves);
    doc.write_changes(&[
        EditChange::AddDatumPlane {
            object: datum,
            name: None,
            plane: DatumPlane::Face {
                result: s.keep,
                face: family,
                offset_mm: Expression::constant(0.).expect("offset"),
            },
        },
        EditChange::AddSketch {
            object: attached_profile,
            name: None,
            sketch: circle(datum, [10., 5.], 1.).expect("attached sketch"),
        },
        EditChange::AddBody {
            object: ObjectId::new(),
            name: None,
            feature_object: attachment,
            feature_name: None,
            feature: extrusion(attached_profile, SolidOperation::NewBody, 1.).expect("attachment"),
        },
        EditChange::AddDatumPlane {
            object: start,
            name: None,
            plane: DatumPlane::Fixed { placement },
        },
        EditChange::AddSketch {
            object: profile,
            name: None,
            sketch: regions,
        },
        EditChange::AddBody {
            object: ObjectId::new(),
            name: None,
            feature_object: feature,
            feature_name: None,
            feature: FeatureAddition::Extrude {
                profile,
                operation: SolidOperation::NewBody,
                regions: vec![],
                reversed: false,
                end_condition: EndCondition::UpToFace {
                    result: s.keep,
                    face: family,
                    offset_mm: Expression::constant(0.).expect("offset"),
                },
            },
        },
    ])
    .expect("set family consumers");
    let built = build(&doc, &mut kernel);
    measure(&built, attachment, &mut kernel, 1, std::f64::consts::PI);
    measure(&built, feature, &mut kernel, 2, 20. * std::f64::consts::PI);
    warm(&doc, &built, &mut kernel);
    built.release_all(&mut kernel);
}
