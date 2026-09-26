// SPDX-License-Identifier: MIT
//! Coordinate-only editing of the persisted, ordered Line polygon.
use std::collections::BTreeSet;

use ferritecad_types::{CadError, ObjectId, Result, StableEntityId, Transform};

use crate::{
    Dependency, DependencyRole, Document, ObjectPayload, ObjectRecord, Point2, PolygonExtrusion,
    RevolveAxis, RevolveExtent, SavedCutTool, SketchCurve, SketchGeometry, SolidOperation,
    editable_extrude,
};

/// The start of this persisted curve; its predecessor's end is the same vertex.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchVertex {
    pub curve_id: StableEntityId,
    pub start_mm: [f64; 2],
}

/// Which saved feature turns an editable Line profile into a solid, and so
/// which numeric policy a coordinate edit must satisfy.
///
/// Stated, never inferred: a Revolve is not an Extrude of some height, and an
/// Extrude has no axis. Each variant carries only what its own policy needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SketchProfileUse {
    /// One forward Blind Extrude/NewBody of this literal height (§25B), or the
    /// base of a validated circular Cut history (§26F/§26I).
    BlindExtrude { feature: ObjectId, height_mm: f64 },
    /// One full turn about the sketch Y axis, NewBody (§27A); edited by §27B.
    ///
    /// `axis_segment` is what the Revolve states (§27C): `None` for a part
    /// with a bore, or the saved Line a solid part closes on. An edit keeps
    /// it, so a draft is checked against it rather than against whatever
    /// class its numbers happen to fall in.
    FullTurnRevolve {
        feature: ObjectId,
        body: ObjectId,
        axis_segment: Option<StableEntityId>,
    },
}

impl SketchProfileUse {
    /// The profile policy of this use, applied to candidate starts in saved
    /// order. The same value creation and the evaluator use, so an edit can
    /// never publish what either refuses.
    fn check(&self, vertices: &[SketchVertex]) -> Result<Vec<Point2>> {
        Ok(match *self {
            Self::BlindExtrude { height_mm, .. } => {
                PolygonExtrusion::new(vertices.iter().map(|v| v.start_mm).collect(), height_mm)?
                    .points()
                    .to_vec()
            }
            Self::FullTurnRevolve { axis_segment, .. } => {
                let lines: Vec<_> = vertices.iter().map(|v| (v.curve_id, v.start_mm)).collect();
                crate::stated_revolution(axis_segment, &lines)?
                    .points()
                    .to_vec()
            }
        })
    }
}

/// A row in objects() order. Refusal is local; copy_access still takes priority.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchChoice {
    pub sketch: ObjectId,
    pub name: Option<String>,
    /// None for an unsupported sketch, never an invented/reordered contour.
    pub vertices: Option<Vec<SketchVertex>>,
    /// Present exactly when `vertices` is: the feature the profile feeds.
    pub profile_use: Option<SketchProfileUse>,
    /// Present only for the original base of a validated nonempty Cut history.
    pub cut_history: Option<SketchCutHistory>,
    pub refusal: Option<String>,
}

/// Coordinate constraints of a validated base, carried with the pinned catalogue.
/// Tools remain in absolute XY coordinates; changing the plate never moves them.
#[derive(Debug, Clone, PartialEq)]
pub struct SketchCutHistory {
    pub body: ObjectId,
    pub base_feature: ObjectId,
    /// All tools in predecessor order, including the final Cut.
    pub tools: Vec<SavedCutTool>,
    /// The saved outer wall this Sketch draws today.
    pub boundary: crate::CutBoundary,
}

pub fn sketch_choices(document: &Document, objects: &[ObjectRecord]) -> Vec<SketchChoice> {
    let history = crate::cut_edit::saved_history(document, objects).map_err(|e| e.to_string());
    choices_with_history(document, objects, history.as_ref())
}

pub(crate) fn choices_with_history(
    document: &Document,
    objects: &[ObjectRecord],
    history: std::result::Result<&crate::cut_edit::CutHistory, &String>,
) -> Vec<SketchChoice> {
    objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
        .map(|o| {
            coordinate_choice(document, objects, o, history).unwrap_or_else(|error| SketchChoice {
                sketch: o.id,
                name: o.name.clone(),
                vertices: None,
                profile_use: None,
                cut_history: None,
                refusal: Some(error.to_string()),
            })
        })
        .collect()
}

fn coordinate_choice(
    document: &Document,
    objects: &[ObjectRecord],
    object: &ObjectRecord,
    history: std::result::Result<&crate::cut_edit::CutHistory, &String>,
) -> Result<SketchChoice> {
    // Standalone polygons keep their complete original contract. Permission for
    // a history comes only from the validated reader, never an object count.
    let mut choice = SketchChoice {
        sketch: object.id,
        name: object.name.clone(),
        vertices: None,
        profile_use: None,
        cut_history: None,
        refusal: None,
    };
    let checked = (|| {
        // §27B: a Sketch that a Revolve turns is judged by the Revolve frame
        // alone. The document says which feature uses the profile; the
        // Extrude frame is never tried as a fallback, nor the other way round.
        if objects
            .iter()
            .any(|o| matches!(&o.payload, ObjectPayload::Revolve(r) if r.profile == object.id))
        {
            let (sketch, profile_use) = revolve_frame(document, objects, object)?;
            return Ok((lines(sketch, &profile_use, false)?, profile_use));
        }
        if let Ok(history) = history {
            let target = &history.target;
            if !target.tools.is_empty() {
                if object.id != target.profile {
                    return Err(unsupported(
                        "coordinate editing of a Cut history requires its original base Sketch",
                    ));
                }
                let ObjectPayload::Sketch(sketch) = &object.payload else {
                    return Err(unsupported("selected object is not a Sketch"));
                };
                let profile_use = SketchProfileUse::BlindExtrude {
                    feature: target.base_feature,
                    height_mm: target.height_mm,
                };
                let vertices = lines(sketch, &profile_use, false)?;
                choice.cut_history = Some(SketchCutHistory {
                    body: target.body,
                    base_feature: target.base_feature,
                    tools: target.tools.clone(),
                    boundary: target.boundary.clone(),
                });
                return Ok((vertices, profile_use));
            }
        }
        let (sketch, height, feature) = extrude_frame(document, objects, object)?;
        let profile_use = SketchProfileUse::BlindExtrude {
            feature,
            height_mm: height,
        };
        Ok((lines(sketch, &profile_use, false)?, profile_use))
    })();
    let (vertices, profile_use) = checked?;
    choice.vertices = Some(vertices);
    choice.profile_use = Some(profile_use);
    Ok(choice)
}

fn unsupported(message: &str) -> CadError {
    CadError::unsupported(message)
}

pub(crate) fn supported(
    document: &Document,
    objects: &[ObjectRecord],
    object: &ObjectRecord,
    allow_constraints: bool,
) -> Result<(Vec<SketchVertex>, f64)> {
    let (sketch, height, feature) = extrude_frame(document, objects, object)?;
    let profile_use = SketchProfileUse::BlindExtrude {
        feature,
        height_mm: height,
    };
    Ok((lines(sketch, &profile_use, allow_constraints)?, height))
}

/// The structure every copy edit of a saved profile requires, and the height
/// the profile is extruded by.
///
/// One place, because the Line editor and the circle editor demand exactly the
/// same document around the sketch and differ only in what the sketch holds.
/// Splitting this would let the two drift, and a class one editor accepted and
/// the other refused would be a class nobody checked.
pub(crate) fn frame<'a>(
    document: &Document,
    objects: &'a [ObjectRecord],
    object: &'a ObjectRecord,
) -> Result<(&'a crate::Sketch, f64)> {
    let (sketch, height, _) = extrude_frame(document, objects, object)?;
    Ok((sketch, height))
}

/// [`frame`], also naming the extrusion it found.
fn extrude_frame<'a>(
    document: &Document,
    objects: &'a [ObjectRecord],
    object: &'a ObjectRecord,
) -> Result<(&'a crate::Sketch, f64, ObjectId)> {
    // Refuse unknown fields/noncanonical envelopes in the one payload we will
    // rewrite, rather than silently discard bytes this reader did not retain.
    require_lossless_payload(object)?;
    // A bounded class, not an attempt to simplify a larger model. For a large
    // unsupported catalogue this returns before any per-object SQL reads.
    if objects.len() != 4 || objects.iter().any(|o| o.parent.is_some()) {
        return Err(unsupported(
            "sketch edit requires exactly one XY plane, Sketch, Blind Extrude and Body",
        ));
    }
    let ObjectPayload::Sketch(sketch) = &object.payload else {
        return Err(unsupported("selected object is not a Sketch"));
    };
    let plane = objects
        .iter()
        .find(|o| o.id == sketch.plane)
        .ok_or_else(|| unsupported("missing sketch plane"))?;
    if !matches!(&plane.payload, ObjectPayload::DatumPlane(p) if p.placement == Transform::IDENTITY)
    {
        return Err(unsupported(
            "sketch edit requires the untransformed XY plane",
        ));
    }
    let extrude = objects.iter().find(|o| matches!(&o.payload, ObjectPayload::Extrude(e)
        if e.profile == object.id && !e.reversed && e.operation == SolidOperation::NewBody && e.target_body.is_none()))
        .ok_or_else(|| unsupported("sketch edit requires one forward Blind Extrude/NewBody"))?;
    let body = objects
        .iter()
        .find(|o| matches!(&o.payload, ObjectPayload::Body(b) if b.tip_feature == Some(extrude.id)))
        .ok_or_else(|| unsupported("sketch edit requires the extrusion's single Body"))?;
    let height = editable_extrude(document, extrude)?;
    let expected = BTreeSet::from([
        Dependency {
            dependent: object.id,
            dependency: plane.id,
            role: DependencyRole::Plane,
        },
        Dependency {
            dependent: extrude.id,
            dependency: object.id,
            role: DependencyRole::Profile,
        },
        Dependency {
            dependent: body.id,
            dependency: extrude.id,
            role: DependencyRole::BodyTip,
        },
    ]);
    if document
        .dependencies()?
        .into_iter()
        .collect::<BTreeSet<_>>()
        != expected
    {
        return Err(unsupported(
            "sketch edit requires only plane, profile and body-tip dependencies",
        ));
    }
    Ok((sketch, height, extrude.id))
}

/// The §27A document around a Sketch that a Revolve turns: the only class a
/// Revolve profile edit accepts (§27B).
///
/// The same shape as the Extrude frame — four root objects, the untransformed
/// XY plane, one feature, its Body and exactly three dependencies — with the
/// Revolve's own intent checked instead of a height. Kept beside [`frame`]
/// rather than folded into it, because the circle, annulus and constraint
/// editors that call `frame` are Extrude-only and must stay so.
fn revolve_frame<'a>(
    document: &Document,
    objects: &'a [ObjectRecord],
    object: &'a ObjectRecord,
) -> Result<(&'a crate::Sketch, SketchProfileUse)> {
    require_lossless_payload(object)?;
    if objects.len() != 4 || objects.iter().any(|o| o.parent.is_some()) {
        return Err(unsupported(
            "Revolve profile edit requires exactly one XY plane, Sketch, Revolve and Body",
        ));
    }
    let ObjectPayload::Sketch(sketch) = &object.payload else {
        return Err(unsupported("selected object is not a Sketch"));
    };
    let plane = objects
        .iter()
        .find(|o| o.id == sketch.plane)
        .ok_or_else(|| unsupported("missing sketch plane"))?;
    if !matches!(&plane.payload, ObjectPayload::DatumPlane(p) if p.placement == Transform::IDENTITY)
    {
        return Err(unsupported(
            "Revolve profile edit requires the untransformed XY plane",
        ));
    }
    let mut revolves = objects.iter().filter_map(|o| match &o.payload {
        ObjectPayload::Revolve(r) if r.profile == object.id => Some((o, r)),
        _ => None,
    });
    let (Some((feature, revolve)), None) = (revolves.next(), revolves.next()) else {
        return Err(unsupported(
            "Revolve profile edit requires exactly one Revolve of this Sketch",
        ));
    };
    // §27D: a sector's coordinates are not edited in this slice. Said by
    // name, before the generic refusal below, so the reason is the real one.
    if let RevolveExtent::Partial { degrees } = revolve.extent {
        return Err(unsupported(&format!(
            "coordinate editing of a partial Revolve ({}° sector) is not supported in this build; \
             only a full-turn Revolve profile can be edited",
            degrees.degrees()
        )));
    }
    // Named one by one rather than compared with a default: an axis, angle or
    // operation a later build adds is refused here, never edited as if it
    // were the full turn about Y this slice measured.
    if !matches!(revolve.axis, RevolveAxis::SketchY)
        || !matches!(revolve.extent, RevolveExtent::FullTurn)
        || revolve.operation != SolidOperation::NewBody
    {
        return Err(unsupported(
            "Revolve profile edit requires a full turn about the sketch Y axis, NewBody",
        ));
    }
    let body = objects
        .iter()
        .find(|o| matches!(&o.payload, ObjectPayload::Body(b) if b.tip_feature == Some(feature.id)))
        .ok_or_else(|| unsupported("Revolve profile edit requires the Revolve's single Body"))?;
    let expected = BTreeSet::from([
        Dependency {
            dependent: object.id,
            dependency: plane.id,
            role: DependencyRole::Plane,
        },
        Dependency {
            dependent: feature.id,
            dependency: object.id,
            role: DependencyRole::Profile,
        },
        Dependency {
            dependent: body.id,
            dependency: feature.id,
            role: DependencyRole::BodyTip,
        },
    ]);
    if document
        .dependencies()?
        .into_iter()
        .collect::<BTreeSet<_>>()
        != expected
    {
        return Err(unsupported(
            "Revolve profile edit requires only plane, profile and body-tip dependencies",
        ));
    }
    Ok((
        sketch,
        SketchProfileUse::FullTurnRevolve {
            feature: feature.id,
            body: body.id,
            axis_segment: revolve.axis_segment,
        },
    ))
}

fn lines(
    sketch: &crate::Sketch,
    profile_use: &SketchProfileUse,
    allow_constraints: bool,
) -> Result<Vec<SketchVertex>> {
    if (!allow_constraints && !sketch.constraints.is_empty())
        || sketch.curves.len() < 3
        || sketch.curves.len() > PolygonExtrusion::MAX_POINTS
    {
        return Err(unsupported(
            "sketch edit requires 3..256 unconstrained Line segments",
        ));
    }
    let mut vertices = Vec::new();
    let mut ends = Vec::new();
    let mut ids = BTreeSet::new();
    for curve in &sketch.curves {
        if curve.construction || !ids.insert(curve.id) {
            return Err(unsupported(
                "construction or duplicate curve IDs are unsupported",
            ));
        }
        let SketchGeometry::Line { start, end } = curve.geometry else {
            return Err(unsupported("sketch edit supports only Lines"));
        };
        vertices.push(SketchVertex {
            curve_id: curve.id,
            start_mm: [start.x, start.y],
        });
        ends.push([end.x, end.y]);
    }
    for (i, end) in ends.iter().enumerate() {
        if *end != vertices[(i + 1) % vertices.len()].start_mm {
            return Err(unsupported(
                "saved Lines must be an exactly closed sequence in stored order",
            ));
        }
    }
    profile_use
        .check(&vertices)
        .map_err(|e| unsupported(&format!("saved polygon is outside edit policy: {e}")))?;
    Ok(vertices)
}

fn require_lossless_payload(object: &ObjectRecord) -> Result<()> {
    if object.payload.to_storage_bytes()?.as_slice() != object.storage_bytes() {
        return Err(unsupported(
            "selected Sketch storage cannot be rewritten losslessly by this reader",
        ));
    }
    Ok(())
}

/// Prepare only the selected payload. Every curve and object ID is retained.
pub fn replace_sketch_coordinates(
    document: &Document,
    selected: ObjectId,
    vertices: &[SketchVertex],
) -> Result<ObjectRecord> {
    let objects = document.objects()?;
    let mut object = objects
        .iter()
        .find(|o| o.id == selected)
        .cloned()
        .ok_or_else(|| CadError::input("selected Sketch UUID does not exist"))?;
    let history = crate::cut_edit::saved_history(document, &objects).map_err(|e| e.to_string());
    let choice = coordinate_choice(document, &objects, &object, history.as_ref())?;
    let points = choice.validate_coordinates(vertices)?;
    let ObjectPayload::Sketch(sketch) = &mut object.payload else {
        unreachable!("checked Sketch")
    };
    for (i, curve) in sketch.curves.iter_mut().enumerate() {
        curve.geometry = SketchGeometry::Line {
            start: points[i],
            end: points[(i + 1) % vertices.len()],
        };
    }
    Ok(object)
}

impl SketchChoice {
    /// The same draft check used before copying. No SQLite or kernel work.
    /// Returns the checked starts, in saved order, under the policy of the
    /// feature this profile feeds.
    pub fn validate_coordinates(&self, vertices: &[SketchVertex]) -> Result<Vec<Point2>> {
        let original = self
            .vertices
            .as_deref()
            .ok_or_else(|| unsupported("unsupported Sketch choice"))?;
        let profile_use = self
            .profile_use
            .ok_or_else(|| unsupported("missing profile feature"))?;
        let points = validate_coordinates(original, &profile_use, vertices)?;
        if let Some(history) = &self.cut_history {
            let SketchProfileUse::BlindExtrude { height_mm, .. } = profile_use else {
                return Err(unsupported("a Cut history base is always an extrusion"));
            };
            let curves = coordinate_curves(vertices, &points);
            crate::cut_edit::validate_base(&curves, height_mm, &history.tools)?;
        }
        Ok(points)
    }
}
fn coordinate_curves(vertices: &[SketchVertex], points: &[Point2]) -> Vec<SketchCurve> {
    vertices
        .iter()
        .enumerate()
        .map(|(i, v)| SketchCurve {
            id: v.curve_id,
            construction: false,
            geometry: SketchGeometry::Line {
                start: points[i],
                end: points[(i + 1) % vertices.len()],
            },
        })
        .collect()
}

/// Extract only the request numbers. Writer re-derivation compares the entire
/// payload, so changed ends, construction, plane or constraints cannot sneak in.
pub(crate) fn prepared_vertices(object: &ObjectRecord) -> Result<Vec<SketchVertex>> {
    let ObjectPayload::Sketch(sketch) = &object.payload else {
        return Err(CadError::input(
            "coordinate preparation must contain a Sketch",
        ));
    };
    sketch
        .curves
        .iter()
        .map(|c| {
            let SketchGeometry::Line { start, .. } = c.geometry else {
                return Err(CadError::input("coordinate preparation must contain Lines"));
            };
            Ok(SketchVertex {
                curve_id: c.id,
                start_mm: [start.x, start.y],
            })
        })
        .collect()
}

fn validate_coordinates(
    original: &[SketchVertex],
    profile_use: &SketchProfileUse,
    vertices: &[SketchVertex],
) -> Result<Vec<Point2>> {
    if vertices.len() != original.len()
        || vertices
            .iter()
            .zip(original)
            .any(|(a, b)| a.curve_id != b.curve_id)
    {
        return Err(CadError::input(
            "request must contain every saved curve UUID exactly once in saved order",
        ));
    }
    let points = profile_use.check(vertices)?;
    let old: Vec<_> = original
        .iter()
        .map(|v| Point2 {
            x: v.start_mm[0],
            y: v.start_mm[1],
        })
        .collect();
    if winding(&old) != winding(&points) {
        return Err(CadError::input("sketch edit cannot change winding"));
    }
    Ok(points)
}

fn winding(points: &[Point2]) -> bool {
    let a = points[0];
    (1..points.len() - 1)
        .map(|i| {
            (points[i].x - a.x) * (points[i + 1].y - a.y)
                - (points[i].y - a.y) * (points[i + 1].x - a.x)
        })
        .sum::<f64>()
        > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_selected_payload_fields_refuse_instead_of_disappearing() {
        use crate::{Envelope, Sketch, SketchCurve};
        let root = tempfile::tempdir().expect("directory");
        let path = root.path().join("source.fcad");
        let mut d = Document::create(&path).expect("doc");
        let id = ObjectId::new();
        let payload = ObjectPayload::Sketch(Sketch {
            plane: ObjectId::new(),
            constraints: vec![],
            curves: vec![SketchCurve {
                id: StableEntityId::new(),
                construction: false,
                geometry: SketchGeometry::Line {
                    start: Point2::ORIGIN,
                    end: Point2::new(1., 0.).expect("point"),
                },
            }],
        });
        d.write(|w| w.put_object(id, None, 0, None, &payload))
            .expect("store");
        require_lossless_payload(&d.object(id).expect("row").expect("record"))
            .expect("ordinary encoding");
        d.close().expect("close");
        let mut envelope =
            Envelope::from_bytes(&payload.to_storage_bytes().expect("bytes")).expect("envelope");
        let mut value: ciborium::value::Value =
            ciborium::from_reader(envelope.payload.as_slice()).expect("CBOR");
        value
            .as_map_mut()
            .expect("map")
            .push(("future_detail".into(), "must not disappear".into()));
        envelope.payload.clear();
        ciborium::into_writer(&value, &mut envelope.payload).expect("CBOR");
        let bytes = envelope.to_bytes().expect("envelope");
        let raw = rusqlite::Connection::open(&path).expect("SQL");
        raw.execute(
            "UPDATE objects SET payload=?1,payload_hash=?2 WHERE id=?3",
            rusqlite::params![
                bytes,
                ferritecad_types::ContentHash::of_bytes(&bytes)
                    .as_bytes()
                    .as_slice(),
                id.to_bytes().as_slice()
            ],
        )
        .expect("future field");
        drop(raw);
        let before = std::fs::read(&path).expect("source");
        let d = Document::open_read_only(&path).expect("still readable");
        let record = d.object(id).expect("row").expect("record");
        let error = require_lossless_payload(&record).expect_err("must refuse loss");
        assert!(error.to_string().contains("losslessly"));
        d.close().expect("close");
        assert_eq!(std::fs::read(&path).expect("source"), before);
    }
}
