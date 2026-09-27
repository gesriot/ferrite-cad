// SPDX-License-Identifier: MIT
//! Changing the radius of the one saved Fillet, in a new copy (§28B).
//!
//! The feature stays the feature: the same UUID, the same producer and joint,
//! the same seven names. The only model change is `radius_mm` in its payload.
//! The history is read again from the saved document on every discovery,
//! preparation and write, through the one frame reader the Cut editors and
//! §28A share ([`crate::cut_edit::saved_history_under_fillet`]); the radius
//! answers to §28A's policy ([`crate::FilletCorner::check_radius`]).
use ferritecad_types::{CadError, ContentHash, ObjectId, Result, StableEntityId};

use crate::cut_edit::{same_meaning, saved_history_under_fillet};
use crate::fillet::{fillet_references, rectangle_corners};
use crate::{Document, Fillet, FilletCorner, FilletEdge, ObjectPayload, ObjectRecord};

/// The saved Fillet a radius edit changes, exactly as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedFillet {
    pub feature: ObjectId,
    pub body: ObjectId,
    /// The base Extrude: the feature consumed and the producer of the edge.
    pub previous: ObjectId,
    pub edge: FilletEdge,
    /// The corner the edge was swept from, with its labels and bound.
    pub corner: FilletCorner,
    pub profile: ObjectId,
    pub height_mm: f64,
    pub radius_mm: f64,
}

impl SavedFillet {
    /// The radius policy this Fillet's corner answers to.
    pub fn check_radius(&self, radius_mm: f64) -> Result<()> {
        self.corner.check_radius(radius_mm)
    }

    /// §28D: this Fillet's corner on a candidate profile of the same Lines,
    /// judged exactly as the saved one is: an axis-aligned rectangle
    /// ([`rectangle_corners`]), the saved joint still one of its corners
    /// ([`crate::corner_for`], by the two Line UUIDs alone) and the saved
    /// radius still inside §28A's policy there. Nothing is clamped.
    pub fn corner_on(&self, curves: &[crate::SketchCurve]) -> Result<FilletCorner> {
        let candidate = crate::Sketch {
            plane: self.profile,
            curves: curves.to_vec(),
            constraints: Vec::new(),
        };
        let corners = rectangle_corners(self.previous, &candidate)?;
        let corner = crate::corner_for(&corners, self.edge)?;
        corner.check_radius(self.radius_mm)?;
        Ok(corner)
    }
}

/// One saved Fillet of a reading: editable, or the reason it is not.
#[derive(Debug, Clone, PartialEq)]
pub struct FilletRadiusChoice {
    pub feature: ObjectId,
    pub name: Option<String>,
    /// The stored payload, reported whatever the frame says.
    pub stored: Fillet,
    pub saved: Option<SavedFillet>,
    pub refusal: Option<String>,
}

impl FilletRadiusChoice {
    /// The same check preparation applies, with no SQLite or kernel work.
    pub fn validate(&self, radius_mm: f64) -> Result<()> {
        let saved = self
            .saved
            .as_ref()
            .ok_or_else(|| CadError::unsupported(self.refusal.clone().unwrap_or_default()))?;
        saved.check_radius(radius_mm)
    }
}

/// The class, read from the saved history around one Fillet object. Shared
/// by the radius edit and the base height edit (§28C), which change different
/// rows of the same frame.
pub(crate) fn saved_fillet(
    document: &Document,
    objects: &[ObjectRecord],
    fillet: &ObjectRecord,
) -> Result<SavedFillet> {
    let ObjectPayload::Fillet(stored) = &fillet.payload else {
        return Err(CadError::input(format!(
            "object {} is {}, not a Fillet",
            fillet.id,
            fillet.payload.type_name()
        )));
    };
    let fillets = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Fillet(_)))
        .count();
    if fillets != 1 {
        return Err(CadError::unsupported(format!(
            "this slice edits a plate with one Fillet, and this document holds {fillets}"
        )));
    }
    let history = saved_history_under_fillet(document, objects, fillet)?;
    if !history.cuts.is_empty() {
        return Err(CadError::unsupported(
            "this slice edits a plate with a Fillet and no Cut",
        ));
    }
    let target = history.target;
    if stored.previous != target.base_feature || stored.edge.feature != target.base_feature {
        return Err(CadError::unsupported(
            "this slice edits a Fillet that rounds an edge of the plate's own Extrude",
        ));
    }
    let profile = objects
        .iter()
        .find(|o| o.id == target.profile)
        .ok_or_else(|| CadError::unsupported("the part's profile is missing"))?;
    let ObjectPayload::Sketch(sketch) = &profile.payload else {
        return Err(CadError::unsupported("the part's profile is not a Sketch"));
    };
    let corners = rectangle_corners(target.base_feature, sketch)?;
    let corner = crate::corner_for(&corners, stored.edge)?;
    corner.check_radius(stored.radius_mm)?;
    // Exactly the seven names §28A gives this Fillet, by meaning, and no other
    // name owned by it. Their UUIDs are what the copy keeps.
    let wanted = fillet_references(
        fillet.id,
        target.base_feature,
        stored.edge.joint,
        &target.profile_segments,
    );
    let mut owned: Vec<_> = document
        .topology_refs()?
        .into_iter()
        .filter(|r| r.owner == fillet.id || r.producer_feature == fillet.id)
        .collect();
    for want in &wanted {
        let at = owned
            .iter()
            .position(|r| same_meaning(r, want))
            .ok_or_else(|| {
                CadError::unsupported(
                    "the saved Fillet does not name the faces this build gives a Fillet",
                )
            })?;
        owned.remove(at);
    }
    if !owned.is_empty() {
        return Err(CadError::unsupported(
            "the saved Fillet names more faces than this build gives a Fillet",
        ));
    }
    Ok(SavedFillet {
        feature: fillet.id,
        body: target.body,
        previous: stored.previous,
        edge: stored.edge,
        corner,
        profile: target.profile,
        height_mm: target.height_mm,
        radius_mm: stored.radius_mm,
    })
}

/// The Fillet that tips the document's plate: `None` when the document holds
/// no Fillet, the §28B frame when it holds, and the frame's reason otherwise.
pub(crate) fn fillet_over_plate(
    document: &Document,
    objects: &[ObjectRecord],
) -> Result<Option<SavedFillet>> {
    let Some(fillet) = objects
        .iter()
        .find(|o| matches!(o.payload, ObjectPayload::Fillet(_)))
    else {
        return Ok(None);
    };
    saved_fillet(document, objects, fillet).map(Some)
}

/// One row per saved Fillet, each editable or with its reason, from one
/// snapshot. Kernel-free.
pub fn fillet_radius_choices(
    document: &Document,
    objects: &[ObjectRecord],
) -> Vec<FilletRadiusChoice> {
    objects
        .iter()
        .filter_map(|o| match &o.payload {
            ObjectPayload::Fillet(stored) => Some((o, stored.clone())),
            _ => None,
        })
        .map(|(o, stored)| {
            let saved = saved_fillet(document, objects, o).map_err(|e| e.to_string());
            FilletRadiusChoice {
                feature: o.id,
                name: o.name.clone(),
                stored,
                refusal: saved.as_ref().err().cloned(),
                saved: saved.ok(),
            }
        })
        .collect()
}

/// A checked radius edit: the selected Fillet row with only its radius
/// replaced, the saved facts it was read with, and the complete version of
/// the document it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedFilletRadius {
    pub(crate) feature: ObjectRecord,
    pub(crate) saved: SavedFillet,
    pub(crate) source_version: ContentHash,
}

impl PreparedFilletRadius {
    pub fn feature(&self) -> &ObjectRecord {
        &self.feature
    }
    /// The Fillet as saved, before the edit.
    pub fn saved(&self) -> &SavedFillet {
        &self.saved
    }
    pub fn radius_mm(&self) -> f64 {
        match &self.feature.payload {
            ObjectPayload::Fillet(f) => f.radius_mm,
            _ => f64::NAN,
        }
    }
    /// The Line UUIDs of the rounded corner, in canonical order.
    pub fn joint(&self) -> [StableEntityId; 2] {
        self.saved.edge.joint.segments()
    }
}

pub fn prepare_fillet_radius(
    document: &Document,
    feature: ObjectId,
    radius_mm: f64,
) -> Result<PreparedFilletRadius> {
    let objects = document.objects()?;
    let mut record = objects
        .iter()
        .find(|o| o.id == feature)
        .cloned()
        .ok_or_else(|| {
            CadError::input(format!("feature {feature} does not exist in this document"))
        })?;
    let saved = saved_fillet(document, &objects, &record)?;
    saved.check_radius(radius_mm)?;
    let ObjectPayload::Fillet(fillet) = &mut record.payload else {
        unreachable!("checked Fillet")
    };
    fillet.radius_mm = radius_mm;
    Ok(PreparedFilletRadius {
        feature: record,
        saved,
        source_version: document.content_version()?,
    })
}

/// The writer's check, against the snapshot the write consumes: the same
/// document version, and the same prepared value derived again from it.
pub(crate) fn rederive(document: &Document, prepared: &PreparedFilletRadius) -> Result<()> {
    if document.content_version()? != prepared.source_version {
        return Err(CadError::input(
            "document changed after Fillet radius preparation",
        ));
    }
    let ObjectPayload::Fillet(fillet) = &prepared.feature.payload else {
        return Err(CadError::input(
            "prepared Fillet radius edit must carry a Fillet",
        ));
    };
    let checked = prepare_fillet_radius(document, prepared.feature.id, fillet.radius_mm)?;
    if checked != *prepared {
        return Err(CadError::input(
            "prepared Fillet radius edit does not describe the current document",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::{
        Body, CapSide, DatumPlane, Dependency, DependencyRole, EdgeFillet, EndCondition,
        EntityKind, Expression, Extrude, Point2, SelectionRule, SemanticRole, Sketch, SketchCurve,
        SketchGeometry, SolidOperation, TopologyRef,
    };
    use ferritecad_types::{ErrorKind, Transform};

    const PLATE: [[f64; 2]; 4] = [[-4.5, 3.25], [33., 3.25], [33., 15.5], [-4.5, 15.5]];

    /// A plate with one §28A Fillet on its second corner, written with the
    /// shipped preparation and writer and no kernel.
    fn filleted(radius: f64) -> (tempfile::TempDir, Document, ObjectId) {
        let root = tempfile::tempdir().expect("dir");
        let mut d = Document::create(root.path().join("plate.fcad")).expect("document");
        let [plane, profile, extrude, body] = std::array::from_fn(|_| ObjectId::new());
        let segments: Vec<StableEntityId> = (0..4).map(|_| StableEntityId::new()).collect();
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
                        id: segments[i],
                        construction: false,
                        geometry: SketchGeometry::Line {
                            start: Point2::new(PLATE[i][0], PLATE[i][1])?,
                            end: Point2::new(PLATE[(i + 1) % 4][0], PLATE[(i + 1) % 4][1])?,
                        },
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            w.put_object(
                profile,
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
                    profile,
                    end_condition: EndCondition::Blind {
                        distance: Expression::constant(6.75)?,
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
                (profile, plane, DependencyRole::Plane),
                (extrude, profile, DependencyRole::Profile),
                (body, extrude, DependencyRole::BodyTip),
            ] {
                w.add_dependency(Dependency {
                    dependent,
                    dependency,
                    role,
                })?;
            }
            for side in [CapSide::Start, CapSide::End] {
                w.put_topology_ref(&TopologyRef {
                    id: StableEntityId::new(),
                    owner: extrude,
                    producer_feature: extrude,
                    expected_kind: EntityKind::Face,
                    output_role: SemanticRole::ExtrudeCap { side },
                    selection: SelectionRule::Exact,
                    fallback_signature: None,
                })?;
            }
            Ok(())
        })
        .expect("plate");
        let target = crate::fillet_choices(&d, &d.objects().expect("objects"))[0]
            .target
            .clone()
            .expect("a target");
        let fillet = EdgeFillet {
            edge: FilletEdge {
                feature: target.base_feature,
                joint: target.corners[1].joint,
            },
            radius_mm: radius,
        };
        let prepared = crate::prepare_edge_fillet(&d, body, &fillet).expect("prepared");
        let id = prepared.feature().id;
        d.write_edge_fillet(&prepared).expect("written");
        (root, d, id)
    }

    type Rows = Vec<Vec<rusqlite::types::Value>>;
    fn cells(d: &Document) -> std::collections::BTreeMap<String, (Vec<String>, Rows)> {
        let c = rusqlite::Connection::open(d.path()).expect("sqlite");
        let names: Vec<String> = c
            .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
            .expect("names")
            .query_map([], |r| r.get(0))
            .expect("names")
            .collect::<rusqlite::Result<_>>()
            .expect("names");
        names
            .into_iter()
            .map(|name| {
                let mut stmt = c
                    .prepare(&format!("SELECT * FROM \"{name}\" ORDER BY 1,2"))
                    .expect("table");
                let n = stmt.column_count();
                let columns = (0..n)
                    .map(|i| stmt.column_name(i).expect("column").to_owned())
                    .collect();
                let rows = stmt
                    .query_map([], |r| (0..n).map(|i| r.get(i)).collect())
                    .expect("rows")
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .expect("rows");
                (name, (columns, rows))
            })
            .collect()
    }

    /// Every cell, except one row's payload and hash and the modification
    /// stamp, is the same before and after: the Fillet's for a radius edit,
    /// the base Extrude's for a height edit (§28C).
    fn only_this_row_moved(
        before: &std::collections::BTreeMap<String, (Vec<String>, Rows)>,
        after: &std::collections::BTreeMap<String, (Vec<String>, Rows)>,
        row: ObjectId,
    ) -> usize {
        let selected = rusqlite::types::Value::Blob(row.to_bytes().to_vec());
        let mut moved = 0;
        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>()
        );
        for (table, (columns, rows)) in before {
            let (theirs, mine) = &after[table];
            assert_eq!(columns, theirs);
            assert_eq!(rows.len(), mine.len(), "{table} rows");
            for (a, b) in rows.iter().zip(mine) {
                for (i, column) in columns.iter().enumerate() {
                    if a[i] == b[i] {
                        continue;
                    }
                    moved += 1;
                    let allowed = match (table.as_str(), column.as_str()) {
                        ("objects", "payload" | "payload_hash") => a.contains(&selected),
                        ("meta", "modified_at") => true,
                        _ => false,
                    };
                    assert!(allowed, "{table}.{column} changed");
                }
            }
        }
        moved
    }

    #[test]
    fn only_the_radius_changes_and_repeated_or_equal_edits_keep_every_identity() {
        let (_root, mut d, fillet) = filleted(2.375);
        let choices = fillet_radius_choices(&d, &d.objects().expect("objects"));
        assert_eq!(choices.len(), 1);
        let choice = &choices[0];
        assert_eq!(choice.refusal, None);
        let saved = choice.saved.clone().expect("editable");
        assert_eq!(saved.radius_mm, 2.375);
        assert_eq!(saved.corner.corner_mm, [33., 3.25]);
        assert_eq!(saved.corner.max_radius_mm, 6.125);
        choice.validate(6.125).expect("the bound is inclusive");
        for bad in [0.0, -1.0, 0.009, 6.126, f64::NAN, f64::INFINITY] {
            assert_eq!(
                choice.validate(bad).expect_err("outside").kind(),
                ErrorKind::Input,
                "{bad}"
            );
        }

        let mut before = cells(&d);
        let refs = d.topology_refs().expect("refs");
        for (radius, moved) in [(4.8125, true), (1.1875, true), (1.1875, false)] {
            let prepared = prepare_fillet_radius(&d, fillet, radius).expect("prepared");
            assert_eq!(prepared.saved().feature, fillet);
            assert_eq!(prepared.radius_mm(), radius);
            d.write_fillet_radius(&prepared).expect("written");
            let after = cells(&d);
            let changed = only_this_row_moved(&before, &after, fillet);
            // A changed radius moves the payload, its hash and the stamp; the
            // same radius moves at most the stamp.
            if moved {
                assert!(changed >= 2, "{changed}");
            } else {
                assert!(changed <= 1, "{changed}");
            }
            before = after;
            let ObjectPayload::Fillet(stored) =
                &d.object(fillet).expect("read").expect("the Fillet").payload
            else {
                panic!("a Fillet");
            };
            assert_eq!(stored.radius_mm, radius);
            assert_eq!(stored.edge, saved.edge);
            assert_eq!(stored.previous, saved.previous);
            assert_eq!(d.topology_refs().expect("refs"), refs, "no name moved");
            assert!(d.validate().expect("validate").is_ok());
        }
    }

    #[test]
    fn the_writer_refuses_a_forged_or_stale_preparation() {
        let (_root, mut d, fillet) = filleted(2.375);
        let honest = prepare_fillet_radius(&d, fillet, 3.0).expect("prepared");
        let mut forged = Vec::new();
        let mut p = honest.clone();
        if let ObjectPayload::Fillet(f) = &mut p.feature.payload {
            f.radius_mm = 6.2;
        }
        forged.push(("an out-of-policy radius", p));
        let mut p = honest.clone();
        if let ObjectPayload::Fillet(f) = &mut p.feature.payload {
            let [_, b] = f.edge.joint.segments();
            f.edge.joint =
                ferritecad_types::ProfileJoint::new(b, StableEntityId::new()).expect("joint");
        }
        forged.push(("another edge", p));
        let mut p = honest.clone();
        if let ObjectPayload::Fillet(f) = &mut p.feature.payload {
            f.previous = ObjectId::new();
        }
        forged.push(("another predecessor", p));
        let mut p = honest.clone();
        p.feature.name = Some("Renamed".to_owned());
        forged.push(("another name", p));
        let mut p = honest.clone();
        p.source_version = ContentHash::of_bytes(b"another version");
        forged.push(("another version", p));
        let before = cells(&d);
        for (why, p) in &forged {
            assert!(d.write_fillet_radius(p).is_err(), "{why} was written");
            assert_eq!(cells(&d), before, "{why} wrote something");
        }
        // Stale: the document changes after preparation.
        let objects = d.objects().expect("objects");
        let first = objects.first().expect("an object").clone();
        d.write(|w| {
            w.put_object(
                first.id,
                first.parent,
                first.ordinal,
                Some("changed after preparation"),
                &first.payload,
            )
            .map(|_| ())
        })
        .expect("change");
        let stale = d.write_fillet_radius(&honest).expect_err("stale");
        assert!(stale.to_string().contains("changed"), "{stale}");
    }

    #[test]
    fn a_fillet_outside_the_frame_is_refused_with_its_reason() {
        // An extra name owned by the Fillet.
        let (_root, mut d, fillet) = filleted(2.375);
        d.write(|w| {
            w.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: fillet,
                producer_feature: fillet,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::FilletFace {
                    source_edge: StableEntityId::new(),
                },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            })
        })
        .expect("extra name");
        let choice = &fillet_radius_choices(&d, &d.objects().expect("objects"))[0];
        assert!(choice.saved.is_none());
        assert!(
            choice
                .refusal
                .as_deref()
                .expect("reason")
                .contains("more faces"),
            "{:?}",
            choice.refusal
        );
        assert!(prepare_fillet_radius(&d, fillet, 3.0).is_err());

        // A name removed from the Fillet.
        let (_root, d, fillet) = filleted(2.375);
        let first = d
            .topology_refs()
            .expect("refs")
            .into_iter()
            .find(|r| r.owner == fillet)
            .expect("a Fillet name");
        rusqlite::Connection::open(d.path())
            .expect("sqlite")
            .execute(
                "DELETE FROM topology_refs WHERE id = ?1",
                [first.id.to_bytes().to_vec()],
            )
            .expect("drop a name");
        assert!(prepare_fillet_radius(&d, fillet, 3.0).is_err());

        // A feature that is not the Fillet, and one that does not exist.
        let (_root, d, fillet) = filleted(2.375);
        let base = match &d.object(fillet).expect("read").expect("Fillet").payload {
            ObjectPayload::Fillet(f) => f.previous,
            _ => panic!("a Fillet"),
        };
        assert_eq!(
            prepare_fillet_radius(&d, base, 3.0)
                .expect_err("an Extrude")
                .kind(),
            ErrorKind::Input
        );
        assert_eq!(
            prepare_fillet_radius(&d, ObjectId::new(), 3.0)
                .expect_err("nothing")
                .kind(),
            ErrorKind::Input
        );

        // A Body whose tip is not the Fillet.
        let (_root, mut d, fillet) = filleted(2.375);
        let body = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
            .expect("Body");
        d.write(|w| {
            w.put_object(
                body.id,
                body.parent,
                body.ordinal,
                body.name.as_deref(),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(base),
                }),
            )
            .map(|_| ())
        })
        .expect("move the tip");
        let choice = &fillet_radius_choices(&d, &d.objects().expect("objects"))[0];
        assert!(
            choice
                .refusal
                .as_deref()
                .expect("reason")
                .contains("not the Body's tip"),
            "{:?}",
            choice.refusal
        );
        assert!(prepare_fillet_radius(&d, fillet, 3.0).is_err());

        // A document with no Fillet offers none.
        let (_root, d, _) = filleted(2.375);
        let objects: Vec<_> = d
            .objects()
            .expect("objects")
            .into_iter()
            .filter(|o| !matches!(o.payload, ObjectPayload::Fillet(_)))
            .collect();
        assert!(fillet_radius_choices(&d, &objects).is_empty());
    }

    fn base_of(d: &Document, fillet: ObjectId) -> ObjectId {
        match &d.object(fillet).expect("read").expect("Fillet").payload {
            ObjectPayload::Fillet(f) => f.previous,
            _ => panic!("a Fillet"),
        }
    }

    fn stored_fillet(d: &Document, fillet: ObjectId) -> Fillet {
        match &d.object(fillet).expect("read").expect("Fillet").payload {
            ObjectPayload::Fillet(f) => f.clone(),
            _ => panic!("a Fillet"),
        }
    }

    fn height_of(d: &Document, base: ObjectId) -> f64 {
        match &d.object(base).expect("read").expect("Extrude").payload {
            ObjectPayload::Extrude(Extrude {
                end_condition: EndCondition::Blind { distance },
                ..
            }) => distance.value(),
            _ => panic!("a Blind Extrude"),
        }
    }

    /// §28C: the plate under the Fillet takes a new height, alone; the Fillet
    /// row, every name and every other cell stay. Height and radius edits
    /// interleave on the same UUIDs.
    #[test]
    fn the_base_height_changes_alone_and_interleaves_with_the_radius() {
        let (_root, mut d, fillet) = filleted(2.375);
        let base = base_of(&d, fillet);
        let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        assert_eq!(reading.unavailable_reason(), None);
        let choice = reading
            .features
            .iter()
            .find(|f| f.feature == base)
            .expect("the base row");
        assert_eq!(choice.refusal, None);
        assert_eq!(choice.cut_history, None, "a Fillet is not a Cut history");
        let context = choice.fillet.clone().expect("the Fillet as context");
        assert_eq!(context.feature, fillet);
        assert_eq!(context.radius_mm, 2.375);
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                choice.validate_distance(bad).expect_err("outside").kind(),
                ErrorKind::Input,
                "{bad}"
            );
        }
        // No bound but a positive height: h < r is a plate like any other.
        choice.validate_distance(0.40625).expect("h < r");

        let refs = d.topology_refs().expect("refs");
        let mut before = cells(&d);
        let mut radius = 2.375;
        for (step, height, moved) in [
            ("up", 9.5, true),
            ("below r", 0.40625, true),
            ("the same", 0.40625, false),
            ("radius", 0.0, true),
            ("after the radius", 13.1875, true),
        ] {
            let row = if step == "radius" {
                radius = 4.8125;
                let p = prepare_fillet_radius(&d, fillet, radius).expect("radius");
                assert_eq!(p.saved().previous, base);
                d.write_fillet_radius(&p).expect("radius written");
                fillet
            } else {
                let p = crate::prepare_extrude_height(&d, base, height).expect(step);
                assert_eq!(p.history(), None, "{step}");
                let saved = p.fillet().expect("the Fillet kept");
                assert_eq!((saved.feature, saved.radius_mm), (fillet, radius));
                d.write_extrude_height(&p).expect(step);
                assert_eq!(height_of(&d, base), height, "{step}");
                base
            };
            let after = cells(&d);
            let changed = only_this_row_moved(&before, &after, row);
            if moved {
                assert!(changed >= 2, "{step}: {changed}");
            } else {
                assert!(changed <= 1, "{step}: {changed}");
            }
            before = after;
            let stored = stored_fillet(&d, fillet);
            assert_eq!(stored.previous, base, "{step}");
            assert_eq!(stored.edge, context.edge, "{step}");
            assert_eq!(stored.radius_mm, radius, "{step}");
            assert_eq!(
                d.topology_refs().expect("refs"),
                refs,
                "{step}: no name moved"
            );
            assert!(d.validate().expect("validate").is_ok(), "{step}");
        }
    }

    /// §28C: the writer re-derives the whole preparation. A forged row, a
    /// preparation that hides the Fillet (which would take the legacy writer
    /// and its weaker reference promise), another Fillet context, another
    /// version and a stale document are all refused and write nothing.
    #[test]
    fn the_height_writer_refuses_a_forged_or_stale_preparation() {
        let (_root, mut d, fillet) = filleted(2.375);
        let base = base_of(&d, fillet);
        let honest = crate::prepare_extrude_height(&d, base, 9.5).expect("prepared");
        let mut forged = Vec::new();
        let mut p = honest.clone();
        p.fillet = None;
        forged.push(("a preparation without the Fillet", p));
        let mut p = honest.clone();
        if let Some(f) = &mut p.fillet {
            f.radius_mm = 3.0;
        }
        forged.push(("another Fillet radius", p));
        let mut p = honest.clone();
        if let Some(f) = &mut p.fillet {
            f.feature = ObjectId::new();
        }
        forged.push(("another Fillet", p));
        let mut p = honest.clone();
        if let ObjectPayload::Extrude(e) = &mut p.feature.payload {
            e.reversed = true;
        }
        forged.push(("a reversed plate", p));
        let mut p = honest.clone();
        if let ObjectPayload::Extrude(e) = &mut p.feature.payload {
            e.profile = ObjectId::new();
        }
        forged.push(("another profile", p));
        let mut p = honest.clone();
        p.feature.name = Some("Renamed".to_owned());
        forged.push(("another name", p));
        let mut p = honest.clone();
        p.feature.id = fillet;
        forged.push(("the Fillet's row", p));
        let mut p = honest.clone();
        p.source_version = ContentHash::of_bytes(b"another version");
        forged.push(("another version", p));
        let before = cells(&d);
        for (why, p) in &forged {
            assert!(d.write_extrude_height(p).is_err(), "{why} was written");
            assert_eq!(cells(&d), before, "{why} wrote something");
        }
        // Stale: a name of the Fillet disappears after preparation.
        let name = d
            .topology_refs()
            .expect("refs")
            .into_iter()
            .find(|r| r.owner == fillet)
            .expect("a Fillet name");
        rusqlite::Connection::open(d.path())
            .expect("sqlite")
            .execute(
                "DELETE FROM topology_refs WHERE id = ?1",
                [name.id.to_bytes().to_vec()],
            )
            .expect("drop a name");
        let before = cells(&d);
        assert!(d.write_extrude_height(&honest).is_err(), "stale");
        assert_eq!(cells(&d), before, "stale wrote something");
        assert!(
            crate::prepare_extrude_height(&d, base, 9.5).is_err(),
            "frame"
        );
    }

    /// §28C refusals: another feature, a bad height, and a Fillet outside the
    /// frame. The blanket refusal stays on every editor but the height.
    #[test]
    fn a_height_under_a_fillet_outside_the_frame_is_refused_with_its_reason() {
        let (_root, d, fillet) = filleted(2.375);
        let base = base_of(&d, fillet);
        for bad in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                crate::prepare_extrude_height(&d, base, bad)
                    .expect_err("a height")
                    .kind(),
                ErrorKind::Input,
                "{bad}"
            );
        }
        assert_eq!(
            crate::prepare_extrude_height(&d, fillet, 9.5)
                .expect_err("the Fillet")
                .kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(
            crate::prepare_extrude_height(&d, ObjectId::new(), 9.5)
                .expect_err("nothing")
                .kind(),
            ErrorKind::Input
        );
        let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        // The base Sketch is the Sketch edit's (§28D); the constraint editor
        // still refuses the filleted plate by name.
        assert!(reading.sketches.iter().all(|s| s.fillet.is_some()));
        assert!(reading.constraint_sketches.iter().all(|s| {
            s.refusal
                .as_deref()
                .is_some_and(|r| r.contains(&fillet.to_string()))
        }));
        assert!(reading.cut_bodies.iter().all(|c| c.refusal.is_some()));

        // An extra name owned by the Fillet: the height is refused too, by
        // the same reason the radius edit gives.
        let (_root, mut d, fillet) = filleted(2.375);
        let base = base_of(&d, fillet);
        d.write(|w| {
            w.put_topology_ref(&TopologyRef {
                id: StableEntityId::new(),
                owner: fillet,
                producer_feature: fillet,
                expected_kind: EntityKind::Face,
                output_role: SemanticRole::FilletFace {
                    source_edge: StableEntityId::new(),
                },
                selection: SelectionRule::Exact,
                fallback_signature: None,
            })
        })
        .expect("extra name");
        let error = crate::prepare_extrude_height(&d, base, 9.5).expect_err("frame");
        assert!(error.to_string().contains("more faces"), "{error}");
        let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        let reason = reading.unavailable_reason().expect("refused");
        assert!(
            reason.contains(&fillet.to_string()) && reason.contains("more faces"),
            "{reason}"
        );
        let row = reading
            .features
            .iter()
            .find(|f| f.feature == base)
            .expect("row");
        assert!(row.fillet.is_none() && row.refusal.is_some());

        // A Body whose tip is not the Fillet.
        let (_root, mut d, fillet) = filleted(2.375);
        let base = base_of(&d, fillet);
        let body = d
            .objects()
            .expect("objects")
            .into_iter()
            .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
            .expect("Body");
        d.write(|w| {
            w.put_object(
                body.id,
                body.parent,
                body.ordinal,
                body.name.as_deref(),
                &ObjectPayload::Body(Body {
                    tip_feature: Some(base),
                }),
            )
            .map(|_| ())
        })
        .expect("move the tip");
        assert!(crate::prepare_extrude_height(&d, base, 9.5).is_err());
    }
}
