// SPDX-License-Identifier: MIT
//! Centre/radius-only editing of the persisted analytic Circle.
//!
//! The same document class the Line editor beside it demands — one XY plane,
//! one Sketch, one forward Blind Extrude/NewBody and its Body — differing only
//! in what the Sketch holds: exactly one unconstrained `Circle`. The circle
//! stays analytic here as it does everywhere else; nothing in this module can
//! turn it into a polygon or mint a new identity for it.
use ferritecad_types::{CadError, ObjectId, Result, StableEntityId};

use crate::{
    CircleExtrusion, Document, ObjectPayload, ObjectRecord, Point2, SketchCurve, SketchGeometry,
    sketch_edit::frame,
};

/// The one saved circle a copy edit can change, exactly as stored.
///
/// The height is reported because it decides what the numbers mean, not
/// because this edit can change it: an edit request carries no height, and the
/// saved one is what the new centre and radius are validated against.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedCircle {
    pub curve_id: StableEntityId,
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
    pub height_mm: f64,
}

/// A row in objects() order. Refusal is local; copy_access still takes priority.
#[derive(Debug, Clone, PartialEq)]
pub struct CircleChoice {
    pub sketch: ObjectId,
    pub name: Option<String>,
    /// None for an unsupported Sketch, never an invented circle.
    pub circle: Option<SavedCircle>,
    pub refusal: Option<String>,
}

/// What one edit asks for: the circle it names, and where it should be.
///
/// The curve UUID is part of the request rather than inferred, so an edit
/// prepared against one reading can never be applied to a different circle
/// that happens to be the only one in some other document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CircleEdit {
    pub curve_id: StableEntityId,
    pub center_mm: [f64; 2],
    pub radius_mm: f64,
}

pub fn circle_choices(document: &Document, objects: &[ObjectRecord]) -> Vec<CircleChoice> {
    objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .map(|o| match supported(document, objects, o) {
            Ok(circle) => CircleChoice {
                sketch: o.id,
                name: o.name.clone(),
                circle: Some(circle),
                refusal: None,
            },
            Err(error) => CircleChoice {
                sketch: o.id,
                name: o.name.clone(),
                circle: None,
                refusal: Some(error.to_string()),
            },
        })
        .collect()
}

fn unsupported(message: &str) -> CadError {
    CadError::unsupported(message)
}

pub(crate) fn supported(
    document: &Document,
    objects: &[ObjectRecord],
    object: &ObjectRecord,
) -> Result<SavedCircle> {
    let (sketch, height) = frame(document, objects, object)?;
    // Circle constraints are not part of this slice, so a constrained circle
    // is refused rather than edited around.
    if !sketch.constraints.is_empty() || sketch.curves.len() != 1 {
        return Err(unsupported(
            "circle edit requires exactly one unconstrained Circle",
        ));
    }
    let (curve_id, center, radius) = analytic_circle(&sketch.curves[0])?;
    // What is already stored has to be inside the policy an edit is judged by,
    // or the first accepted edit would silently narrow the document.
    CircleExtrusion::new([center.x, center.y], radius, height)
        .map_err(|e| unsupported(&format!("saved circle is outside edit policy: {e}")))?;
    Ok(SavedCircle {
        curve_id,
        center_mm: [center.x, center.y],
        radius_mm: radius,
        height_mm: height,
    })
}

/// One stored curve read as a circle a copy edit may move or resize.
///
/// Shared with the annular editor beside this, which needs the same reading
/// twice. Only the reading is shared: how many circles a Sketch must hold, and
/// what the numbers then have to satisfy, is each editor's own question.
pub(crate) fn analytic_circle(curve: &SketchCurve) -> Result<(StableEntityId, Point2, f64)> {
    if curve.construction {
        return Err(unsupported("construction geometry bounds no face"));
    }
    let SketchGeometry::Circle { center, radius } = curve.geometry else {
        return Err(unsupported("circle edit supports only a Circle"));
    };
    Ok((curve.id, center, radius))
}

/// Prepare only the selected payload. The Sketch and Circle IDs are retained.
pub fn replace_circle_geometry(
    document: &Document,
    selected: ObjectId,
    edit: &CircleEdit,
) -> Result<ObjectRecord> {
    let objects = document.objects()?;
    let mut object = objects
        .iter()
        .find(|o| o.id == selected)
        .cloned()
        .ok_or_else(|| CadError::input("selected Sketch UUID does not exist"))?;
    let saved = supported(document, &objects, &object)?;
    let checked = validate(&saved, edit)?;
    let ObjectPayload::Sketch(sketch) = &mut object.payload else {
        unreachable!("checked Sketch")
    };
    // Exactly the geometry of the one named curve. Its identity, the
    // construction flag and every other stored field are left as they are.
    sketch.curves[0].geometry = SketchGeometry::Circle {
        center: checked.center(),
        radius: checked.radius_mm(),
    };
    Ok(object)
}

impl CircleChoice {
    /// The same draft check used before copying. No SQLite or kernel work.
    pub fn validate_circle(&self, edit: &CircleEdit) -> Result<CircleExtrusion> {
        let saved = self
            .circle
            .as_ref()
            .ok_or_else(|| unsupported("unsupported Circle choice"))?;
        validate(saved, edit)
    }
}

/// The one numeric policy, applied to the request against the saved height.
///
/// The height is the document's, not the request's: an edit that could change
/// it would be an extrusion edit wearing a circle's name, and there is already
/// a command for that.
fn validate(saved: &SavedCircle, edit: &CircleEdit) -> Result<CircleExtrusion> {
    if edit.curve_id != saved.curve_id {
        return Err(CadError::input(
            "request must name the saved Circle curve UUID",
        ));
    }
    CircleExtrusion::new(edit.center_mm, edit.radius_mm, saved.height_mm)
}

/// The stored circle of a prepared payload, for a writer that must not trust
/// its caller about which row it is replacing.
pub(crate) fn prepared_circle(object: &ObjectRecord) -> Result<(StableEntityId, Point2, f64)> {
    let ObjectPayload::Sketch(sketch) = &object.payload else {
        return Err(CadError::input("circle preparation must contain a Sketch"));
    };
    let [curve] = sketch.curves.as_slice() else {
        return Err(CadError::input("circle preparation must hold one curve"));
    };
    let SketchGeometry::Circle { center, radius } = curve.geometry else {
        return Err(CadError::input("circle preparation must hold a Circle"));
    };
    if !sketch.constraints.is_empty() {
        return Err(CadError::input("circle editing stores no constraints"));
    }
    Ok((curve.id, center, radius))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Body, DatumPlane, Dependency, DependencyRole, EndCondition, Expression, Extrude, Sketch,
        SketchCurve, SolidOperation,
    };
    use ferritecad_types::Transform;

    /// The smallest document this editor accepts, built the way creation does.
    fn fixture(radius: f64) -> (tempfile::TempDir, Document, ObjectId, StableEntityId) {
        let root = tempfile::tempdir().expect("dir");
        let mut d = Document::create(root.path().join("source.fcad")).expect("document");
        let [plane, sketch, extrude, body] = std::array::from_fn(|_| ObjectId::new());
        let curve = StableEntityId::new();
        d.write(|w| {
            w.put_object(
                plane,
                None,
                0,
                Some("XY"),
                &ObjectPayload::DatumPlane(DatumPlane {
                    placement: Transform::IDENTITY,
                }),
            )?;
            w.put_object(
                sketch,
                None,
                1,
                Some("Profile"),
                &ObjectPayload::Sketch(Sketch {
                    plane,
                    curves: vec![SketchCurve {
                        id: curve,
                        construction: false,
                        geometry: SketchGeometry::Circle {
                            center: Point2::new(12., -7.).expect("finite"),
                            radius,
                        },
                    }],
                    constraints: Vec::new(),
                }),
            )?;
            w.put_object(
                extrude,
                None,
                2,
                Some("Extrude1"),
                &ObjectPayload::Extrude(Extrude {
                    profile: sketch,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(15.)?,
                    },
                    reversed: false,
                    operation: SolidOperation::NewBody,
                    target_body: None,
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
        .expect("fixture");
        (root, d, sketch, curve)
    }

    #[test]
    fn a_saved_circle_is_discovered_with_its_own_identity_and_height() {
        let (_root, d, sketch, curve) = fixture(10.);
        let objects = d.objects().expect("objects");
        let choices = circle_choices(&d, &objects);
        assert_eq!(choices.len(), 1, "one Sketch, one row");
        let choice = &choices[0];
        assert_eq!(choice.sketch, sketch);
        assert_eq!(choice.refusal, None);
        assert_eq!(
            choice.circle,
            Some(SavedCircle {
                curve_id: curve,
                center_mm: [12., -7.],
                radius_mm: 10.,
                height_mm: 15.,
            })
        );
        // The Line catalogue on the same document refuses, as it always did.
        let lines = crate::sketch_choices(&d, &objects);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].vertices.is_none(), "no invented polygon");
        assert!(
            lines[0]
                .refusal
                .as_deref()
                .is_some_and(|r| r.contains("Line segments")),
            "{:?}",
            lines[0].refusal
        );
    }

    #[test]
    fn only_the_named_circle_and_only_its_geometry_is_rewritten() {
        let (_root, d, sketch, curve) = fixture(10.);
        let before = d.object(sketch).expect("row").expect("record");
        let edit = CircleEdit {
            curve_id: curve,
            center_mm: [-3.5, 4.25],
            radius_mm: 6.75,
        };
        let prepared = replace_circle_geometry(&d, sketch, &edit).expect("prepared");
        assert_eq!(prepared.id, before.id);
        assert_eq!(prepared.parent, before.parent);
        assert_eq!(prepared.ordinal, before.ordinal);
        assert_eq!(prepared.name, before.name);
        let (id, center, radius) = prepared_circle(&prepared).expect("circle");
        assert_eq!(id, curve, "the circle keeps its UUID");
        assert_eq!((center.x, center.y, radius), (-3.5, 4.25, 6.75));
        let ObjectPayload::Sketch(s) = &prepared.payload else {
            unreachable!("Sketch")
        };
        assert_eq!(s.plane, {
            let ObjectPayload::Sketch(o) = &before.payload else {
                unreachable!("Sketch")
            };
            o.plane
        });
        assert!(s.constraints.is_empty() && !s.curves[0].construction);
    }

    #[test]
    fn the_circle_writer_refuses_forged_payloads_and_stale_preparations() {
        let (_root, mut d, sketch, curve) = fixture(10.);
        let before = d.content_version().expect("version");
        let edit = CircleEdit {
            curve_id: curve,
            center_mm: [-3.5, 4.25],
            radius_mm: 6.75,
        };
        let prepared = replace_circle_geometry(&d, sketch, &edit).expect("prepare");
        for case in ["curve identity", "plane", "construction", "radius"] {
            let mut forged = prepared.clone();
            let ObjectPayload::Sketch(s) = &mut forged.payload else {
                unreachable!()
            };
            match case {
                "curve identity" => s.curves[0].id = StableEntityId::new(),
                "plane" => s.plane = ObjectId::new(),
                "construction" => s.curves[0].construction = true,
                "radius" => {
                    s.curves[0].geometry = SketchGeometry::Circle {
                        center: Point2::ORIGIN,
                        radius: -1.,
                    }
                }
                _ => unreachable!(),
            }
            assert!(
                d.write_circle_geometry(&forged).is_err(),
                "writer accepted forged {case}"
            );
            assert_eq!(d.content_version().expect("unchanged"), before, "{case}");
        }
        d.write_circle_geometry(&prepared)
            .expect("valid geometry edit");
        let current = d.object(sketch).expect("row").expect("object");
        assert_eq!(current.payload, prepared.payload);
        let edited_version = d.content_version().expect("version");
        assert!(
            d.write_circle_geometry(&prepared).is_err(),
            "stale preparation"
        );
        assert_eq!(d.content_version().expect("unchanged"), edited_version);
    }

    #[test]
    fn a_request_naming_another_curve_or_an_unstorable_number_is_refused() {
        let (_root, d, sketch, curve) = fixture(10.);
        for (edit, what) in [
            (
                CircleEdit {
                    curve_id: StableEntityId::new(),
                    center_mm: [0., 0.],
                    radius_mm: 5.,
                },
                "a foreign curve UUID",
            ),
            (
                CircleEdit {
                    curve_id: curve,
                    center_mm: [0., 0.],
                    radius_mm: 0.,
                },
                "a radius of nothing",
            ),
            (
                CircleEdit {
                    curve_id: curve,
                    center_mm: [0., 0.],
                    radius_mm: -1.,
                },
                "a negative radius",
            ),
            (
                CircleEdit {
                    curve_id: curve,
                    center_mm: [f64::NAN, 0.],
                    radius_mm: 5.,
                },
                "a centre that is not a number",
            ),
            (
                CircleEdit {
                    curve_id: curve,
                    center_mm: [0., 0.],
                    radius_mm: f64::INFINITY,
                },
                "an unbounded radius",
            ),
            (
                CircleEdit {
                    curve_id: curve,
                    center_mm: [2e6, 0.],
                    radius_mm: 5.,
                },
                "a centre outside the editor's range",
            ),
        ] {
            assert!(
                replace_circle_geometry(&d, sketch, &edit).is_err(),
                "{what} was accepted"
            );
        }
        // And a Sketch UUID that is not in this document.
        assert!(
            replace_circle_geometry(
                &d,
                ObjectId::new(),
                &CircleEdit {
                    curve_id: curve,
                    center_mm: [0., 0.],
                    radius_mm: 5.,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn a_sketch_this_editor_does_not_own_is_refused_rather_than_rewritten() {
        // Two curves, a Line, a construction circle and a constrained circle
        // are each outside the class, and say which.
        let (_root, mut d, sketch, curve) = fixture(10.);
        let circle = SketchCurve {
            id: curve,
            construction: false,
            geometry: SketchGeometry::Circle {
                center: Point2::ORIGIN,
                radius: 4.,
            },
        };
        for (curves, constraints, expect) in [
            (
                vec![
                    circle.clone(),
                    SketchCurve {
                        id: StableEntityId::new(),
                        construction: false,
                        geometry: SketchGeometry::Circle {
                            center: Point2::new(30., 0.).expect("finite"),
                            radius: 4.,
                        },
                    },
                ],
                vec![],
                "exactly one",
            ),
            (
                vec![SketchCurve {
                    construction: true,
                    ..circle.clone()
                }],
                vec![],
                "construction",
            ),
            (
                vec![SketchCurve {
                    geometry: SketchGeometry::Line {
                        start: Point2::ORIGIN,
                        end: Point2::new(10., 0.).expect("finite"),
                    },
                    ..circle
                }],
                vec![],
                "only a Circle",
            ),
        ] {
            let mut object = d.object(sketch).expect("row").expect("record");
            let ObjectPayload::Sketch(s) = &mut object.payload else {
                unreachable!("Sketch")
            };
            s.curves = curves;
            s.constraints = constraints;
            d.write(|w| {
                w.put_object(
                    object.id,
                    object.parent,
                    object.ordinal,
                    object.name.as_deref(),
                    &object.payload,
                )
            })
            .expect("store");
            let objects = d.objects().expect("objects");
            let error = supported(
                &d,
                &objects,
                &d.object(sketch).expect("row").expect("record"),
            )
            .expect_err("outside the class");
            assert!(error.to_string().contains(expect), "{error} != {expect}");
            assert!(circle_choices(&d, &objects)[0].circle.is_none());
        }
    }
}
