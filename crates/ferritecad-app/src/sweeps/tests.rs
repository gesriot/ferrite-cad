// SPDX-License-Identifier: MIT
use super::*;
use crate::curve_fixtures::{self as fixtures, Drawing, curve, line, point, rectangle};
use ferritecad_document::{
    EditChange, EndCondition, Expression, FeatureAddition, ObjectPayload, RevolveExtent,
    SketchGeometry,
};
use ferritecad_types::StableEntityId;

pub(crate) mod window;

fn feature(change: &EditChange) -> &FeatureAddition {
    match change {
        EditChange::AddBody { feature, .. } | EditChange::AddFeature { feature, .. } => feature,
        _ => panic!("one sweep addition"),
    }
}

#[test]
fn numbered_names_share_all_object_names_and_fill_each_prefix_gap() {
    let mut f = Drawing::new(vec![], false);
    let saved = f.saved();
    for prefix in ["Sketch", "Body", "Extrude", "Revolve"] {
        for n in [1, 3] {
            fixtures::put(
                &mut f.doc,
                ObjectId::new(),
                &format!("{prefix} {n}"),
                ObjectPayload::Sketch(saved.clone()),
            );
        }
    }
    let objects = f.doc.objects().expect("objects");
    for prefix in ["Sketch", "Body", "Extrude", "Revolve"] {
        assert_eq!(
            crate::object_names::next(&[], prefix),
            format!("{prefix} 1")
        );
        assert_eq!(
            crate::object_names::next(&objects, prefix),
            format!("{prefix} 2")
        );
    }
}

#[test]
fn new_body_is_one_add_body_with_two_new_identities_and_free_names() {
    let f = Drawing::new(rectangle(), false);
    for kind in [Kind::Extrude, Kind::Revolve] {
        let changes = Draft::new(kind, f.sketch, None)
            .changes(&f.doc.objects().expect("fixture operation"))
            .expect("fixture operation");
        let [
            EditChange::AddBody {
                object,
                name,
                feature_object,
                feature_name,
                feature,
            },
        ] = &changes[..]
        else {
            panic!("one AddBody")
        };
        assert_ne!(object, feature_object);
        assert_eq!(name.as_deref(), Some("Body 1"));
        assert_eq!(
            feature_name.as_deref(),
            Some(format!("{} 1", kind.label()).as_str())
        );
        assert_eq!(
            feature
                .payload(None)
                .expect("fixture operation")
                .previous_feature(),
            None
        );
    }
}

#[test]
fn add_cut_intersect_each_send_one_feature_to_the_selected_body() {
    let sketch = ObjectId::new();
    let body = ObjectId::new();
    for kind in [Kind::Extrude, Kind::Revolve] {
        for operation in [
            SolidOperation::Add,
            SolidOperation::Cut,
            SolidOperation::Intersect,
        ] {
            let mut draft = Draft::new(kind, sketch, Some(body));
            draft.operation = operation;
            let changes = draft.changes(&[]).expect("fixture operation");
            let [
                EditChange::AddFeature {
                    body: selected,
                    name,
                    feature,
                    ..
                },
            ] = &changes[..]
            else {
                panic!("one AddFeature")
            };
            assert_eq!(*selected, body);
            assert_eq!(
                name.as_deref(),
                Some(format!("{} 1", kind.label()).as_str())
            );
            match feature {
                FeatureAddition::Extrude {
                    operation: saved,
                    profile,
                    regions,
                    ..
                }
                | FeatureAddition::Revolve {
                    operation: saved,
                    profile,
                    regions,
                    ..
                } => {
                    assert_eq!(*saved, operation);
                    assert_eq!(*profile, sketch);
                    assert!(regions.is_empty());
                }
                _ => panic!("sweep"),
            }
        }
    }
}

#[test]
fn extrusion_ends_and_reverse_are_saved_exactly_and_unused_distance_is_not_parsed() {
    let mut draft = Draft::new(Kind::Extrude, ObjectId::new(), Some(ObjectId::new()));
    draft.distance = "12.5".into();
    for (end, expected) in [
        (
            End::Blind,
            EndCondition::Blind {
                distance: Expression::constant(12.5).expect("fixture operation"),
            },
        ),
        (
            End::Symmetric,
            EndCondition::Symmetric {
                distance: Expression::constant(12.5).expect("fixture operation"),
            },
        ),
        (End::ThroughAll, EndCondition::ThroughAll),
        (End::UpToNext, EndCondition::UpToNext),
    ] {
        draft.end = end;
        if matches!(end, End::ThroughAll | End::UpToNext) {
            draft.distance = "unused".into();
        }
        for reversed in [false, true] {
            draft.reversed = reversed;
            assert_eq!(
                feature(&draft.changes(&[]).expect("fixture operation")[0]),
                &FeatureAddition::Extrude {
                    profile: draft.sketch,
                    regions: vec![],
                    operation: SolidOperation::NewBody,
                    reversed,
                    end_condition: expected.clone(),
                }
            );
        }
    }
}

#[test]
fn revolve_axes_full_turn_and_partial_radians_are_exact() {
    let mut draft = Draft::new(Kind::Revolve, ObjectId::new(), None);
    for axis in [
        RevolveAxis::SketchX,
        RevolveAxis::SketchY,
        RevolveAxis::Line(StableEntityId::new()),
    ] {
        draft.axis = axis;
        for full in [true, false] {
            draft.full_turn = full;
            draft.angle = if full { "unused" } else { "90" }.into();
            assert_eq!(
                feature(&draft.changes(&[]).expect("fixture operation")[0]),
                &FeatureAddition::Revolve {
                    profile: draft.sketch,
                    regions: vec![],
                    operation: SolidOperation::NewBody,
                    axis,
                    extent: if full {
                        RevolveExtent::FullTurn
                    } else {
                        RevolveExtent::Partial {
                            angle: Expression::constant(std::f64::consts::FRAC_PI_2)
                                .expect("fixture operation"),
                        }
                    },
                }
            );
        }
    }
}

#[test]
fn partial_angle_uses_saved_angle_bounds_and_complete_errors() {
    let mut draft = Draft::new(Kind::Revolve, ObjectId::new(), None);
    draft.full_turn = false;
    for (text, error) in [
        ("bad", "invalid input: Not a number: \"bad\""),
        (
            "0",
            "invalid input: the revolve angle is not positive: a partial revolve turns \
             right-handed about +Y, and a mirrored sweep is not supported",
        ),
        (
            "360",
            "invalid input: the revolve angle is a full turn, which has no end faces: \
             state a full turn instead of a partial angle",
        ),
        (
            "361",
            "invalid input: the revolve angle is more than one turn: a solid turns at most once",
        ),
        (
            "0.001",
            "invalid input: the revolve angle is below the minimum: the sector would \
             be too thin to keep every face",
        ),
        (
            "359.999",
            "invalid input: the revolve angle is within the minimum sector of a full \
             turn: a partial revolve stops short of that, and a full turn is stated as \
             one",
        ),
    ] {
        draft.angle = text.into();
        assert_eq!(
            draft.changes(&[]).expect_err("invalid input").to_string(),
            error
        );
    }
    for good in ["0.01", "359.99", " 90 "] {
        draft.angle = good.into();
        draft.changes(&[]).expect("same inclusive bounds");
    }
}

#[test]
fn line_axes_include_construction_in_document_order_with_exact_accepted_coordinates() {
    let mut construction = line([1.5, -2.], [3., 4.125]);
    construction.construction = true;
    let plain = line([-8., 9.], [10., -11.]);
    let f = Drawing::new(
        vec![
            curve(SketchGeometry::Point {
                at: point(20., 30.),
            }),
            construction.clone(),
            plain.clone(),
        ],
        false,
    );
    let scene = crate::sessions::tests::tabs::mock_scene(&f.path).expect("fixture operation");
    assert_eq!(
        request::axes(&scene.sketch_presentations[0]),
        vec![
            (RevolveAxis::SketchX, "Sketch X".into()),
            (RevolveAxis::SketchY, "Sketch Y".into()),
            (
                RevolveAxis::Line(construction.id),
                "Line 1: (1.5, -2) → (3, 4.125) mm".into()
            ),
            (
                RevolveAxis::Line(plain.id),
                "Line 2: (-8, 9) → (10, -11) mm".into()
            ),
        ]
    );
}

#[test]
fn invalid_distance_and_absent_body_refuse_without_creating_a_request() {
    let mut draft = Draft::new(Kind::Extrude, ObjectId::new(), None);
    draft.distance = "twelve".into();
    assert_eq!(
        draft.changes(&[]).expect_err("number").to_string(),
        "invalid input: enter a finite distance in mm"
    );
    draft.distance = "NaN".into();
    assert_eq!(
        draft.changes(&[]).expect_err("finite").to_string(),
        "invalid input: model values must be finite, found NaN"
    );
    draft.distance = "10".into();
    draft.operation = SolidOperation::Cut;
    assert_eq!(
        draft.changes(&[]).expect_err("body").to_string(),
        "invalid input: The document has no body to change."
    );
}

#[test]
fn unnamed_sketch_refusal_uses_the_same_object_label_and_preserves_every_reason_word() {
    let f = Drawing::new(rectangle(), false);
    let scene = crate::sessions::tests::tabs::mock_scene(&f.path).expect("scene");
    let mut source = Source::from(scene.edit_source.expect("facts"));
    let object = source
        .objects
        .iter_mut()
        .find(|o| o.id == f.sketch)
        .expect("sketch");
    object.name = None;
    source.profile_failures = vec![(
        f.sketch,
        "invalid input: the complete profile reason".into(),
    )];
    let object = source
        .objects
        .iter()
        .find(|o| o.id == f.sketch)
        .expect("sketch");
    assert_eq!(
        available(&source, object),
        Err("sketch 'Unnamed' gives no profile: invalid input: the complete profile reason".into())
    );
}
