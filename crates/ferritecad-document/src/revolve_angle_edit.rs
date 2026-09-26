// SPDX-License-Identifier: MIT
//! Changing the stated angle of a saved partial Revolve (§27E).
//!
//! The angle is the only intent this edit changes. The profile, the axis, the
//! direction, the operation, the stated axis Line and every identity are
//! kept; a full turn never becomes a sector, nor a sector a full turn.

use ferritecad_types::{CadError, ContentHash, ObjectId, Result, StableEntityId};

use crate::{Document, ObjectPayload, ObjectRecord, RevolveAngle, RevolveAxis, RevolveExtent};

/// One saved Revolve, and whether its angle can be edited, from the same
/// pinned reading as every other catalogue.
#[derive(Debug, Clone, PartialEq)]
pub struct RevolveAngleChoice {
    pub feature: ObjectId,
    pub name: Option<String>,
    /// The Body whose tip this Revolve is, when the Revolve is editable.
    pub body: Option<ObjectId>,
    /// The saved angle of a sector, in degrees; `None` for a full turn.
    pub degrees: Option<f64>,
    /// §27C: the saved Line on the axis of a solid sector; `None` for one
    /// with a bore. Kept by the edit.
    pub axis_segment: Option<StableEntityId>,
    pub refusal: Option<String>,
}

impl RevolveAngleChoice {
    /// The document-domain check the form and the preparation share: this
    /// Revolve's own refusal first, then the one angle policy.
    pub fn validate_angle(&self, degrees: f64) -> Result<RevolveAngle> {
        if let Some(refusal) = &self.refusal {
            return Err(CadError::unsupported(refusal.clone()));
        }
        RevolveAngle::new(degrees)
    }
}

pub fn revolve_angle_choices(
    document: &Document,
    objects: &[ObjectRecord],
) -> Vec<RevolveAngleChoice> {
    objects
        .iter()
        .filter_map(|object| {
            let ObjectPayload::Revolve(revolve) = &object.payload else {
                return None;
            };
            let found = angle_frame(document, objects, object);
            Some(RevolveAngleChoice {
                feature: object.id,
                name: object.name.clone(),
                body: found.as_ref().ok().copied(),
                degrees: match revolve.extent {
                    RevolveExtent::Partial { degrees } => Some(degrees.degrees()),
                    RevolveExtent::FullTurn => None,
                },
                axis_segment: revolve.axis_segment,
                refusal: found.err().map(|e| e.to_string()),
            })
        })
        .collect()
}

/// The class an angle edit accepts, around the Revolve `object`: the shared
/// standalone Revolve frame, a partial turn about the sketch Y axis starting
/// a body, and a profile of the class the Revolve states. Returns its Body.
fn angle_frame(
    document: &Document,
    objects: &[ObjectRecord],
    object: &ObjectRecord,
) -> Result<ObjectId> {
    let ObjectPayload::Revolve(revolve) = &object.payload else {
        return Err(CadError::unsupported(format!(
            "object {} is {}, not a Revolve",
            object.id,
            object.payload.type_name()
        )));
    };
    // The one payload this edit rewrites: bytes this reader would not write
    // back are refused rather than silently dropped.
    if object.payload.to_storage_bytes()?.as_slice() != object.storage_bytes() {
        return Err(CadError::unsupported(
            "selected Revolve storage cannot be rewritten losslessly by this reader",
        ));
    }
    let sketch = objects
        .iter()
        .find(|o| o.id == revolve.profile && matches!(o.payload, ObjectPayload::Sketch(_)))
        .ok_or_else(|| CadError::unsupported("the Revolve's profile is not a Sketch"))?;
    let frame = crate::sketch_edit::revolve_document(
        document,
        objects,
        sketch,
        "Revolve angle edit",
        |revolve| {
            if revolve.extent == RevolveExtent::FullTurn {
                return Err(CadError::unsupported(
                    "a full-turn Revolve has no angle to edit; changing between a full turn and \
                     a partial angle is not supported",
                ));
            }
            if !matches!(revolve.axis, RevolveAxis::SketchY)
                || revolve.operation != crate::SolidOperation::NewBody
            {
                return Err(CadError::unsupported(
                    "Revolve angle edit requires a partial turn about the sketch Y axis, NewBody",
                ));
            }
            Ok(())
        },
    )?;
    if frame.feature.id != object.id {
        return Err(CadError::unsupported(
            "Revolve angle edit requires the Sketch's only Revolve",
        ));
    }
    // The same profile class check discovery, the evaluator and every other
    // Revolve reader apply, on the same objects.
    let choice = crate::revolve_choices(document, objects)
        .into_iter()
        .find(|c| c.feature == object.id)
        .ok_or_else(|| CadError::unsupported("the Revolve is not in the catalogue"))?;
    if let Some(refusal) = choice.profile_refusal {
        return Err(CadError::unsupported(refusal));
    }
    Ok(frame.body.id)
}

/// A checked angle edit: the selected Revolve row with only its angle
/// replaced, and the complete version of the document it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedRevolveAngle {
    pub(crate) feature: ObjectRecord,
    pub(crate) source_version: ContentHash,
}

impl PreparedRevolveAngle {
    pub fn feature(&self) -> &ObjectRecord {
        &self.feature
    }
}

pub fn prepare_revolve_angle(
    document: &Document,
    feature: ObjectId,
    degrees: f64,
) -> Result<PreparedRevolveAngle> {
    let objects = document.objects()?;
    let mut record = objects
        .iter()
        .find(|o| o.id == feature)
        .cloned()
        .ok_or_else(|| {
            CadError::input(format!("feature {feature} does not exist in this document"))
        })?;
    angle_frame(document, &objects, &record)?;
    let angle = RevolveAngle::new(degrees)?;
    let ObjectPayload::Revolve(revolve) = &mut record.payload else {
        unreachable!("checked Revolve")
    };
    revolve.extent = RevolveExtent::Partial { degrees: angle };
    Ok(PreparedRevolveAngle {
        feature: record,
        source_version: document.content_version()?,
    })
}

/// The writer's check, against the snapshot the write consumes: the same
/// document version, and the same prepared value derived again from it.
pub(crate) fn rederive(document: &Document, prepared: &PreparedRevolveAngle) -> Result<()> {
    if document.content_version()? != prepared.source_version {
        return Err(CadError::input(
            "document changed after Revolve angle preparation",
        ));
    }
    let ObjectPayload::Revolve(revolve) = &prepared.feature.payload else {
        return Err(CadError::input(
            "prepared Revolve angle edit must carry a Revolve",
        ));
    };
    let RevolveExtent::Partial { degrees } = revolve.extent else {
        return Err(CadError::input(
            "prepared Revolve angle edit must carry a partial angle",
        ));
    };
    let checked = prepare_revolve_angle(document, prepared.feature.id, degrees.degrees())?;
    if checked != *prepared {
        return Err(CadError::input(
            "prepared Revolve angle edit does not describe the current document",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::{
        Body, DatumPlane, Dependency, DependencyRole, Point2, Revolve, Sketch, SketchCurve,
        SketchGeometry, SolidOperation,
    };
    use ferritecad_types::Transform;

    /// A standalone sector with a bore, written without a kernel.
    fn sector(path: &std::path::Path, extent: RevolveExtent) -> (Document, ObjectId, ObjectId) {
        let mut d = Document::create(path).expect("document");
        let [plane, sketch, revolve, body] = std::array::from_fn(|_| ObjectId::new());
        let points = [[4., 0.], [10., 0.], [10., 15.], [4., 15.]];
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
            let curves = (0..4)
                .map(|i| {
                    Ok(SketchCurve {
                        id: StableEntityId::new(),
                        construction: false,
                        geometry: SketchGeometry::Line {
                            start: Point2::new(points[i][0], points[i][1])?,
                            end: Point2::new(points[(i + 1) % 4][0], points[(i + 1) % 4][1])?,
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
                    profile: sketch,
                    axis: RevolveAxis::SketchY,
                    extent,
                    operation: SolidOperation::NewBody,
                    axis_segment: None,
                }),
            )?;
            w.put_object(
                body,
                None,
                3,
                Some("Body"),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(revolve),
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
        (d, revolve, sketch)
    }

    fn quarter() -> RevolveExtent {
        RevolveExtent::Partial {
            degrees: RevolveAngle::new(90.).expect("angle"),
        }
    }

    fn stored_degrees(d: &Document, revolve: ObjectId) -> f64 {
        match d.object(revolve).expect("row").expect("Revolve").payload {
            ObjectPayload::Revolve(Revolve {
                extent: RevolveExtent::Partial { degrees },
                ..
            }) => degrees.degrees(),
            other => panic!("not a sector: {other:?}"),
        }
    }

    #[test]
    fn catalogue_names_each_refusal_and_prepares_only_the_angle() {
        let root = tempfile::tempdir().expect("directory");
        let (d, revolve, sketch) = sector(&root.path().join("sector.fcad"), quarter());
        let objects = d.objects().expect("objects");
        let [choice] = revolve_angle_choices(&d, &objects)
            .try_into()
            .expect("one row");
        assert_eq!(choice.refusal, None);
        assert_eq!(choice.degrees, Some(90.));
        assert!(choice.body.is_some());
        for bad in [
            0.,
            -90.,
            0.005,
            359.995,
            360.,
            400.,
            f64::NAN,
            f64::INFINITY,
        ] {
            assert!(choice.validate_angle(bad).is_err(), "{bad}");
            assert!(prepare_revolve_angle(&d, revolve, bad).is_err(), "{bad}");
        }
        let prepared = prepare_revolve_angle(&d, revolve, 220.).expect("prepared");
        let before = d.object(revolve).expect("row").expect("Revolve");
        let mut expected = before.payload.clone();
        let ObjectPayload::Revolve(r) = &mut expected else {
            panic!("a Revolve")
        };
        r.extent = RevolveExtent::Partial {
            degrees: RevolveAngle::new(220.).expect("angle"),
        };
        assert_eq!(
            prepared.feature().payload,
            expected,
            "only the angle changes"
        );
        assert_eq!(prepared.feature().id, before.id);
        assert_eq!(prepared.feature().name, before.name);
        let error = prepare_revolve_angle(&d, sketch, 220.).expect_err("a Sketch");
        assert!(error.to_string().contains("not a Revolve"), "{error}");
        let error = prepare_revolve_angle(&d, ObjectId::new(), 220.).expect_err("unknown");
        assert!(error.to_string().contains("does not exist"), "{error}");
        d.close().expect("close");

        let (full, revolve, _) = sector(&root.path().join("full.fcad"), RevolveExtent::FullTurn);
        let objects = full.objects().expect("objects");
        let [choice] = revolve_angle_choices(&full, &objects)
            .try_into()
            .expect("one row");
        assert_eq!(choice.degrees, None);
        let refusal = choice.refusal.clone().expect("a full turn");
        assert!(
            refusal.contains("full-turn Revolve has no angle"),
            "{refusal}"
        );
        assert!(choice.validate_angle(90.).is_err());
        let error = prepare_revolve_angle(&full, revolve, 90.).expect_err("full turn");
        assert!(
            error.to_string().contains("full-turn Revolve has no angle"),
            "{error}"
        );
        full.close().expect("close");
    }

    /// The writer's own gate, reached directly: a prepared value that does
    /// not describe the document it is written to is refused inside the
    /// transaction, and nothing is written.
    #[test]
    fn writer_rederives_and_refuses_forged_or_stale_preparations() {
        let root = tempfile::tempdir().expect("directory");
        let path = root.path().join("sector.fcad");
        let (mut d, revolve, sketch) = sector(&path, quarter());

        // Forged: a field other than the angle.
        let mut forged = prepare_revolve_angle(&d, revolve, 220.).expect("prepared");
        let ObjectPayload::Revolve(r) = &mut forged.feature.payload else {
            panic!("a Revolve")
        };
        r.axis_segment = Some(StableEntityId::new());
        let error = d.write_revolve_angle(&forged).expect_err("forged payload");
        assert!(error.to_string().contains("does not describe"), "{error}");
        // Forged: another row's identity under the same payload.
        let mut forged = prepare_revolve_angle(&d, revolve, 220.).expect("prepared");
        forged.feature.name = Some("Renamed".into());
        let error = d.write_revolve_angle(&forged).expect_err("forged row");
        assert!(error.to_string().contains("does not describe"), "{error}");
        // Forged: a version the document never had.
        let mut forged = prepare_revolve_angle(&d, revolve, 220.).expect("prepared");
        forged.source_version = ContentHash::of_bytes(b"another document");
        let error = d.write_revolve_angle(&forged).expect_err("forged version");
        assert!(error.to_string().contains("changed after"), "{error}");
        assert_eq!(stored_degrees(&d, revolve), 90.);

        // Stale: the source changed after preparation.
        let prepared = prepare_revolve_angle(&d, revolve, 220.).expect("prepared");
        let profile = d.object(sketch).expect("row").expect("Sketch");
        d.write(|w| {
            w.put_object(
                profile.id,
                profile.parent,
                profile.ordinal,
                Some("renamed after preparation"),
                &profile.payload,
            )
        })
        .expect("change");
        let error = d.write_revolve_angle(&prepared).expect_err("stale");
        assert!(error.to_string().contains("changed after"), "{error}");
        assert_eq!(stored_degrees(&d, revolve), 90.);

        // The honest preparation writes exactly the angle.
        let prepared = prepare_revolve_angle(&d, revolve, 220.).expect("prepared");
        d.write_revolve_angle(&prepared).expect("written");
        assert_eq!(stored_degrees(&d, revolve), 220.);
        d.close().expect("close");
    }
}
