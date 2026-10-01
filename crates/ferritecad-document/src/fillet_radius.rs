// SPDX-License-Identifier: MIT
//! Changing the radius of one saved Fillet, in a new copy (§28B, §28H, §28L).
//!
//! The feature stays the feature: the same UUID, the same producer and joint,
//! the same seven names. The only model change is `radius_mm` in its payload.
//! The history is read again from the saved document on every discovery,
//! preparation and write, through the one frame reader the Cut editors and
//! §28A share ([`crate::cut_edit::saved_history_under_fillets`]); the radius
//! answers to §28A's policy ([`crate::FilletCorner::check_radius`]).
use ferritecad_types::{CadError, ContentHash, ObjectId, Result, StableEntityId};

use crate::cut_edit::same_meaning;
use crate::fillet::{corners_of_lines, fillet_references, rectangle_corners};
use crate::{Document, Fillet, FilletCorner, FilletEdge, ObjectPayload, ObjectRecord};

/// The saved Fillet a radius edit changes, exactly as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedFillet {
    pub feature: ObjectId,
    pub body: ObjectId,
    /// The feature this Fillet rounds the result of: the base Extrude for the
    /// first Fillet, the Fillet before it otherwise.
    pub previous: ObjectId,
    pub edge: FilletEdge,
    /// The corner the edge was swept from, with its labels and bound.
    pub corner: FilletCorner,
    pub profile: ObjectId,
    pub height_mm: f64,
    pub radius_mm: f64,
    /// §28E: whether the base Sketch carries constraints. Then `corner` is read
    /// from the **stored** Lines — the solver's starting guess — and its
    /// lengths and bound say nothing about the part: the radius bound is asked
    /// of the solved plate by the rebuild, at every rebuild.
    pub constrained: bool,
    /// §28H/§28L: the place in the history, 1 for the only or the first
    /// Fillet, up to [`crate::MAX_PLATE_FILLETS`]. Each Fillet rounds the
    /// result of the one before it.
    pub history_index: usize,
    /// §28H: the plate's base Extrude, the producer of every Fillet's edge.
    pub base_feature: ObjectId,
    /// §28L: every **other** Fillet of the history, in history order, each
    /// with the Line it shares with this one if the corners are adjacent.
    pub neighbours: Vec<NeighbourFillet>,
}

/// §28H/§28L: another Fillet of the history, beside the one edited.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NeighbourFillet {
    pub feature: ObjectId,
    pub history_index: usize,
    pub edge: FilletEdge,
    /// Its corner on the stored Lines.
    pub corner: FilletCorner,
    pub radius_mm: f64,
    /// The Line the two corners share and its stored length, when adjacent.
    pub shared: Option<(StableEntityId, f64)>,
}

impl SavedFillet {
    /// How many Fillets the whole history holds, this one included.
    pub fn history_len(&self) -> usize {
        self.neighbours.len() + 1
    }

    /// §28H, kept as a projection for histories of exactly two Fillets: the
    /// other one. `None` for one Fillet and — never describing it falsely — for
    /// three or four, whose neighbours are [`Self::neighbours`].
    pub fn neighbour(&self) -> Option<&NeighbourFillet> {
        match self.neighbours.as_slice() {
            [only] => Some(only),
            _ => None,
        }
    }

    /// The radius policy this Fillet's corner answers to. For a constrained
    /// plate only the corner-free part of it ([`crate::check_radius_value`]):
    /// the stored lengths are not the part's, in either direction.
    pub fn check_radius(&self, radius_mm: f64) -> Result<()> {
        if self.constrained {
            return crate::fillet::check_radius_value(radius_mm);
        }
        self.corner.check_radius(radius_mm)?;
        // §28H/§28L: the pair policy, in history order, against **every**
        // other radius as saved: the very check the rebuild applies at the
        // later Fillet of each pair. The first refusal is the earliest
        // neighbour in the history.
        for n in &self.neighbours {
            let beside = (n.feature, n.edge.joint);
            if n.history_index < self.history_index {
                crate::fillet::check_pair_beside(
                    beside,
                    &n.corner,
                    n.radius_mm,
                    &self.corner,
                    radius_mm,
                )?;
            } else {
                crate::fillet::check_pair_beside(
                    beside,
                    &self.corner,
                    radius_mm,
                    &n.corner,
                    n.radius_mm,
                )?;
            }
        }
        Ok(())
    }

    /// The largest radius this Fillet takes without a solve: §28A's bound
    /// and, beside **each** adjacent Fillet, the pair bound by the same
    /// predicate [`Self::check_radius`] applies. The minimum of all of them,
    /// so the radius offered satisfies every neighbour, not the first found.
    /// `None` for a constrained plate.
    pub fn max_radius_mm(&self) -> Option<f64> {
        if self.constrained {
            return None;
        }
        let mut bound = self.corner.max_radius_mm;
        for n in &self.neighbours {
            let Some((_, length)) = n.shared else {
                continue;
            };
            let pair = if n.history_index < self.history_index {
                crate::fillet::pair_bound(length, n.radius_mm)
            } else {
                crate::fillet::pair_bound_of_first(length, n.radius_mm)
            };
            // No radius fits beside a neighbour this wide: nothing is offered.
            bound = bound.min(if pair.is_nan() { 0.0 } else { pair });
        }
        Some(bound)
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
        // §28J: the edge was swept by the base Extrude, which is `previous`
        // only for the first Fillet; a later Fillet's `previous` is the Fillet
        // before it.
        let corners = rectangle_corners(self.base_feature, &candidate)?;
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

/// §28L: the Fillets of the document's plate, in history order, as the one
/// reader reads them — `n` from 1 to [`crate::MAX_PLATE_FILLETS`].
///
/// The exact history (one Body, one base Extrude, an unbranched chain of
/// Fillets over it, no Cut, exactly the plane, profile, predecessor and
/// body-tip dependencies), every Fillet's payload and corner, the radii by the
/// one policy, and every name by meaning (seven per Fillet and one origin face
/// per earlier Fillet) with no other name owned by any of them. Shared by the
/// radius edit, the height, coordinate and constraint edits and the add-Fillet
/// editor; the older one- and two-Fillet shapes are projections of its list.
pub(crate) fn saved_fillets(
    document: &Document,
    objects: &[ObjectRecord],
) -> Result<Vec<SavedFillet>> {
    let records: Vec<&ObjectRecord> = objects
        .iter()
        .filter(|o| matches!(o.payload, ObjectPayload::Fillet(_)))
        .collect();
    if records.is_empty() {
        return Ok(Vec::new());
    }
    if records.len() > crate::MAX_PLATE_FILLETS {
        return Err(CadError::unsupported(format!(
            "this build edits a plate with at most {} Fillets, one per vertical corner, and \
             this document holds {}",
            crate::MAX_PLATE_FILLETS,
            records.len()
        )));
    }
    let payload = |o: &ObjectRecord| match &o.payload {
        ObjectPayload::Fillet(f) => f.clone(),
        _ => unreachable!("filtered Fillets"),
    };
    // The tip of the chain: the one Fillet no other Fillet rounds the result
    // of. The chain below it is then read by `previous`, never by position.
    let tops: Vec<&&ObjectRecord> = records
        .iter()
        .filter(|f| {
            !records
                .iter()
                .any(|o| matches!(&o.payload, ObjectPayload::Fillet(x) if x.previous == f.id))
        })
        .collect();
    let [top] = tops.as_slice() else {
        return Err(CadError::unsupported(
            "this slice edits Fillets that round one plate one after the other, and the \
             Fillets of this document do not form one chain",
        ));
    };
    let (base, earlier) = crate::fillet::chain_below(objects, payload(top).previous)?;
    crate::fillet::refuse_branch(objects, base, &earlier)?;
    let mut chain: Vec<&ObjectRecord> = earlier;
    chain.push(top);
    if chain.len() != records.len() {
        return Err(CadError::unsupported(format!(
            "this slice edits the chain of Fillets under the Body's tip, and {} of the {} \
             Fillets of this document are not in it",
            records.len() - chain.len(),
            records.len()
        )));
    }
    let history = crate::cut_edit::saved_history_under_fillets(document, objects, &chain)?;
    if !history.cuts.is_empty() {
        return Err(CadError::unsupported(
            "this slice edits a plate with Fillets and no Cut",
        ));
    }
    let target = history.target;
    let stored: Vec<Fillet> = chain.iter().map(|o| payload(o)).collect();
    if stored[0].previous != target.base_feature
        || stored.iter().any(|f| f.edge.feature != target.base_feature)
    {
        return Err(CadError::unsupported(
            "this slice edits Fillets that each round an edge of the plate's own Extrude",
        ));
    }
    let profile = objects
        .iter()
        .find(|o| o.id == target.profile)
        .ok_or_else(|| CadError::unsupported("the part's profile is missing"))?;
    let ObjectPayload::Sketch(sketch) = &profile.payload else {
        return Err(CadError::unsupported("the part's profile is not a Sketch"));
    };
    // The stored Lines are the rectangle the Fillets were made on, and they say
    // which two Lines meet at each corner. Only for an unconstrained plate are
    // they also the part, so only then is the radius bound asked of them.
    let constrained = !sketch.constraints.is_empty();
    let corners = corners_of_lines(target.base_feature, &sketch.curves)?;
    let found: Vec<FilletCorner> = stored
        .iter()
        .map(|f| crate::corner_for(&corners, f.edge))
        .collect::<Result<_>>()?;
    for (k, corner) in found.iter().enumerate() {
        if let Some(j) = found[..k].iter().position(|c| c.joint == corner.joint) {
            return Err(CadError::unsupported(format!(
                "Fillets {} and {} round the same corner {}",
                chain[j].id, chain[k].id, corner.joint
            )));
        }
    }
    // The saved radii by the one rule; for a constrained plate only their
    // value, the rest being the solved plate's.
    for (k, f) in stored.iter().enumerate() {
        if constrained {
            crate::fillet::check_radius_value(f.radius_mm)?;
        } else {
            found[k].check_radius(f.radius_mm)?;
            for j in 0..k {
                crate::fillet::check_pair_beside(
                    (chain[j].id, stored[j].edge.joint),
                    &found[j],
                    stored[j].radius_mm,
                    &found[k],
                    f.radius_mm,
                )?;
            }
        }
    }
    // Exactly §28A's seven names for every Fillet and one origin face of every
    // earlier Fillet, by meaning, and no other name owned by any of them.
    let mut wanted = Vec::new();
    for (k, f) in stored.iter().enumerate() {
        wanted.extend(fillet_references(
            chain[k].id,
            target.base_feature,
            f.edge.joint,
            &target.profile_segments,
        ));
        for j in 0..k {
            wanted.push(crate::fillet::origin_fillet_reference(
                chain[k].id,
                &crate::ExistingFillet {
                    feature: chain[j].id,
                    edge: stored[j].edge,
                    radius_mm: stored[j].radius_mm,
                    corner: found[j],
                },
            ));
        }
    }
    let mine = |id: ObjectId| chain.iter().any(|o| o.id == id);
    let mut owned: Vec<_> = document
        .topology_refs()?
        .into_iter()
        .filter(|r| mine(r.owner) || mine(r.producer_feature))
        .collect();
    for want in &wanted {
        let at = owned
            .iter()
            .position(|r| same_meaning(r, want))
            .ok_or_else(|| {
                CadError::unsupported(
                    "the saved Fillets do not name the faces this build gives a history of Fillets",
                )
            })?;
        owned.remove(at);
    }
    if !owned.is_empty() {
        return Err(CadError::unsupported(
            "the saved Fillets name more faces than this build gives a history of Fillets",
        ));
    }
    Ok(chain
        .iter()
        .enumerate()
        .map(|(k, record)| SavedFillet {
            feature: record.id,
            body: target.body,
            previous: stored[k].previous,
            edge: stored[k].edge,
            corner: found[k],
            profile: target.profile,
            height_mm: target.height_mm,
            radius_mm: stored[k].radius_mm,
            constrained,
            history_index: k + 1,
            base_feature: target.base_feature,
            neighbours: chain
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != k)
                .map(|(j, other)| NeighbourFillet {
                    feature: other.id,
                    history_index: j + 1,
                    edge: stored[j].edge,
                    corner: found[j],
                    radius_mm: stored[j].radius_mm,
                    shared: crate::fillet::shared_side(&found[k], &found[j]),
                })
                .collect(),
        })
        .collect())
}

/// §28H: the Fillet whose radius an edit changes: any Fillet of the history
/// the one reader reads, selected by its UUID.
pub(crate) fn saved_fillet_for_radius(
    document: &Document,
    objects: &[ObjectRecord],
    fillet: &ObjectRecord,
) -> Result<SavedFillet> {
    let ObjectPayload::Fillet(_) = &fillet.payload else {
        return Err(CadError::input(format!(
            "object {} is {}, not a Fillet",
            fillet.id,
            fillet.payload.type_name()
        )));
    };
    saved_fillets(document, objects)?
        .into_iter()
        .find(|s| s.feature == fillet.id)
        .ok_or_else(|| {
            CadError::input(format!(
                "object {} is not one of this plate's Fillets",
                fillet.id
            ))
        })
}

/// The Fillets over the plate a base height, coordinate or constraint edit
/// keeps, as they stand in the history, bottom first: the first rounds the
/// base Extrude and each later one rounds the result of the one before it.
/// A plate with one Fillet is §28C's frame exactly.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FilletsOverPlate {
    pub fillets: Vec<SavedFillet>,
}

impl FilletsOverPlate {
    pub fn first(&self) -> &SavedFillet {
        &self.fillets[0]
    }
    pub fn last(&self) -> &SavedFillet {
        &self.fillets[self.fillets.len() - 1]
    }
}

/// §28C–§28L: the Fillets of the plate, `None` when the document holds none,
/// the history when the one reader admits it, and its reason otherwise.
pub(crate) fn fillets_over_plate(
    document: &Document,
    objects: &[ObjectRecord],
) -> Result<Option<FilletsOverPlate>> {
    let fillets = saved_fillets(document, objects)?;
    Ok((!fillets.is_empty()).then_some(FilletsOverPlate { fillets }))
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
            let saved = saved_fillet_for_radius(document, objects, o).map_err(|e| e.to_string());
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
    let saved = saved_fillet_for_radius(document, &objects, &record)?;
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
        filleted_on(PLATE, radius)
    }

    fn filleted_on(plate: [[f64; 2]; 4], radius: f64) -> (tempfile::TempDir, Document, ObjectId) {
        let (root, mut d, body) = bare_on(plate);
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

    /// §28L: the plate alone — plane, profile, Extrude, Body and the Extrude's
    /// two caps — written without a kernel, with the Body's id.
    fn bare_on(plate: [[f64; 2]; 4]) -> (tempfile::TempDir, Document, ObjectId) {
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
                            start: Point2::new(plate[i][0], plate[i][1])?,
                            end: Point2::new(plate[(i + 1) % 4][0], plate[(i + 1) % 4][1])?,
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
        (root, d, body)
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

    /// §28H: [`filleted`] with a second Fillet at stored corner `at` (an
    /// index into the plate's corners), written by the shipped preparation
    /// and writer. Returns the two Fillets in history order.
    fn sequential(
        r1: f64,
        at: usize,
        r2: f64,
    ) -> (tempfile::TempDir, Document, ObjectId, ObjectId) {
        sequential_on(PLATE, r1, at, r2)
    }

    fn sequential_on(
        plate: [[f64; 2]; 4],
        r1: f64,
        at: usize,
        r2: f64,
    ) -> (tempfile::TempDir, Document, ObjectId, ObjectId) {
        let (root, mut d, first) = filleted_on(plate, r1);
        let objects = d.objects().expect("objects");
        let body = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Body(_)))
            .expect("body")
            .id;
        let target = crate::fillet_choices(&d, &objects)[0]
            .target
            .clone()
            .expect("a second target");
        let corner = crate::rectangle_corners(
            target.base_feature,
            &match &objects
                .iter()
                .find(|o| o.id == target.profile)
                .expect("profile")
                .payload
            {
                ObjectPayload::Sketch(s) => s.clone(),
                _ => panic!("a Sketch"),
            },
        )
        .expect("corners")[at];
        let prepared = crate::prepare_edge_fillet(
            &d,
            body,
            &EdgeFillet {
                edge: FilletEdge {
                    feature: target.base_feature,
                    joint: corner.joint,
                },
                radius_mm: r2,
            },
        )
        .expect("second");
        let second = prepared.feature().id;
        d.write_edge_fillet(&prepared).expect("written");
        (root, d, first, second)
    }

    fn radius_of(d: &Document, fillet: ObjectId) -> f64 {
        match &d.object(fillet).expect("read").expect("row").payload {
            ObjectPayload::Fillet(f) => f.radius_mm,
            _ => panic!("a Fillet"),
        }
    }

    /// §28H: either Fillet of the §28G history is editable by its UUID; the
    /// edit moves only that row's payload, its hash and the stamp; every
    /// name, the other Fillet, both predecessors and edges are kept.
    #[test]
    fn either_sequential_radius_changes_alone_and_keeps_every_identity() {
        // Corners 1 and 2 share the 12.25 mm Line x = 33.
        let (_root, mut d, first, second) = sequential(2.375, 2, 3.0625);
        let choices = fillet_radius_choices(&d, &d.objects().expect("objects"));
        assert_eq!(choices.len(), 2);
        for (choice, (id, index, other)) in
            choices.iter().zip([(first, 1, second), (second, 2, first)])
        {
            assert_eq!(choice.refusal, None, "{choice:?}");
            let saved = choice.saved.as_ref().expect("editable");
            assert_eq!(saved.feature, id);
            assert_eq!(saved.history_index, index);
            let n = saved.neighbour().expect("the other Fillet");
            assert_eq!((n.feature, n.history_index), (other, 3 - index));
            let (_, length) = n.shared.expect("adjacent");
            assert_eq!(length, 12.25);
            assert_eq!(
                saved.max_radius_mm(),
                Some(6.125),
                "§28A's bound decides here"
            );
        }
        let refs = d.topology_refs().expect("refs");
        let mut before = cells(&d);
        for (fillet, radius) in [(first, 4.5), (second, 5.0625), (first, 1.25), (second, 2.0)] {
            let other = if fillet == first { second } else { first };
            let other_radius = radius_of(&d, other);
            let prepared = prepare_fillet_radius(&d, fillet, radius).expect("prepared");
            assert_eq!(prepared.saved().feature, fillet);
            d.write_fillet_radius(&prepared).expect("written");
            let after = cells(&d);
            assert!(only_this_row_moved(&before, &after, fillet) >= 2);
            before = after;
            assert_eq!(radius_of(&d, fillet), radius);
            assert_eq!(
                radius_of(&d, other),
                other_radius,
                "the other radius is kept"
            );
            assert_eq!(d.topology_refs().expect("refs"), refs, "no name moved");
            assert!(d.validate().expect("validate").is_ok());
        }
        // The base height (§28I), the base Sketch's coordinates (§28J) and
        // its constraints (§28K) are edited under the two-Fillet history too.
        let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        assert!(reading.fillet_features.iter().all(|c| c.refusal.is_none()));
        assert!(reading.features.iter().all(|f| f.refusal.is_none()));
        assert!(reading.sketches.iter().all(|s| s.refusal.is_none()));
        assert!(
            reading
                .constraint_sketches
                .iter()
                .all(|s| s.refusal.is_none())
        );
    }

    /// §28H: the bound discovery states for either radius is the largest the
    /// one predicate accepts — the one the rebuild applies at the second
    /// Fillet — and the next float is refused by both. Opposite corners get
    /// no pair bound.
    #[test]
    fn the_pair_bound_is_exact_for_either_radius_and_absent_opposite() {
        // The public numeric helper must terminate outside the narrower
        // saved-model domain too. Near cancellation, stepping by one ulp
        // from L-r2-0.01 would traverse quadrillions of floats.
        for (length, second) in [
            (1.0, 0.99),
            (0.03, 0.01),
            (8.0, 4.0),
            (12.25, 6.12),
            (1e12, 0.01),
            (f64::MAX, f64::MAX / 2.0),
        ] {
            let first = crate::pair_bound_of_first(length, second);
            assert!(first.is_finite() && first >= 0.0);
            assert!(second <= crate::pair_bound(length, first));
            assert!(second > crate::pair_bound(length, first.next_up()));
        }
        for (length, second) in [
            (f64::NAN, 1.0),
            (1.0, f64::NAN),
            (f64::INFINITY, 1.0),
            (1.0, f64::INFINITY),
            (-1.0, 0.1),
            (1.0, -0.1),
            (0.0, 0.0),
            (1.0, 2.0),
        ] {
            assert!(crate::pair_bound_of_first(length, second).is_nan());
        }
        // r2 = 6.12 > 12.25 − 6.125 − 0.01, so the pair bound on r1 is
        // tighter than §28A's 6.125.
        let (_root, d, first, second) = sequential(6.0, 2, 6.12);
        let objects = d.objects().expect("objects");
        let choices = fillet_radius_choices(&d, &objects);
        let bound = |id: ObjectId| {
            let c = choices.iter().find(|c| c.feature == id).expect("row");
            (
                c.clone(),
                c.saved
                    .as_ref()
                    .expect("saved")
                    .max_radius_mm()
                    .expect("bound"),
            )
        };
        let lines = match &objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("Sketch")
            .payload
        {
            ObjectPayload::Sketch(s) => s.curves.clone(),
            _ => unreachable!(),
        };
        let stored = |id: ObjectId| match &objects.iter().find(|o| o.id == id).expect("row").payload
        {
            ObjectPayload::Fillet(f) => f.clone(),
            _ => unreachable!(),
        };
        let (c1, max1) = bound(first);
        assert_eq!(max1, crate::pair_bound_of_first(12.25, 6.12));
        assert!(max1 < 6.125);
        c1.validate(max1).expect("the offered bound is accepted");
        assert_eq!(
            c1.validate(max1.next_up())
                .expect_err("the next float")
                .kind(),
            ErrorKind::Input
        );
        // The rebuild's own check at the second Fillet agrees, float for float.
        let mut objects_at = objects.clone();
        for (r1, ok) in [(max1, true), (max1.next_up(), false)] {
            for o in &mut objects_at {
                if o.id == first
                    && let ObjectPayload::Fillet(f) = &mut o.payload
                {
                    f.radius_mm = r1;
                }
            }
            let verdict = crate::evaluable_fillet(&objects_at, &stored(second), Some(&lines));
            assert_eq!(verdict.is_ok(), ok, "{r1}: {verdict:?}");
        }
        let (c2, max2) = bound(second);
        assert_eq!(max2, crate::pair_bound(12.25, 6.0).min(6.125));
        c2.validate(max2).expect("accepted");
        assert!(c2.validate(max2.next_up()).is_err());

        // On an 8 mm side beside r2 = 4 the offered r1 is 3.9900000000000007:
        // the history-order predicate accepts it, while the same inequality
        // written the other way round (r1 ≤ 8 − 4 − 0.01) would refuse it.
        // Editing Fillet 1 is judged in history order, float for float.
        let short = [[-4.5, 3.25], [33., 3.25], [33., 11.25], [-4.5, 11.25]];
        let (_root, d, first, second) = sequential_on(short, 2.375, 2, 4.0);
        let objects = d.objects().expect("objects");
        let choices = fillet_radius_choices(&d, &objects);
        let c1 = choices.iter().find(|c| c.feature == first).expect("row");
        let max1 = c1
            .saved
            .as_ref()
            .expect("saved")
            .max_radius_mm()
            .expect("bound");
        assert_eq!(max1, 3.9900000000000007);
        assert!(
            max1 > crate::pair_bound(8.0, 4.0),
            "the swapped form refuses it"
        );
        c1.validate(max1).expect("accepted in history order");
        assert!(c1.validate(max1.next_up()).is_err());
        let lines = match &objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("Sketch")
            .payload
        {
            ObjectPayload::Sketch(s) => s.curves.clone(),
            _ => unreachable!(),
        };
        let two = match &objects
            .iter()
            .find(|o| o.id == second)
            .expect("row")
            .payload
        {
            ObjectPayload::Fillet(f) => f.clone(),
            _ => unreachable!(),
        };
        let mut objects_at = objects.clone();
        for (r1, ok) in [(max1, true), (max1.next_up(), false)] {
            for o in &mut objects_at {
                if o.id == first
                    && let ObjectPayload::Fillet(f) = &mut o.payload
                {
                    f.radius_mm = r1;
                }
            }
            let verdict = crate::evaluable_fillet(&objects_at, &two, Some(&lines));
            assert_eq!(verdict.is_ok(), ok, "{r1}: {verdict:?}");
        }

        // The opposite corner: §28A's bound alone, for both.
        let (_root, d, first, second) = sequential(6.125, 3, 6.125);
        for c in fillet_radius_choices(&d, &d.objects().expect("objects")) {
            let saved = c.saved.as_ref().expect("editable");
            assert_eq!(saved.neighbour().expect("other").shared, None);
            assert_eq!(saved.max_radius_mm(), Some(6.125));
            c.validate(6.125)
                .expect("the per-corner bound is inclusive");
            assert!(c.validate(6.125f64.next_up()).is_err());
            assert!([first, second].contains(&c.feature));
        }
    }

    /// The object rows of `d` whose payload differs from `before`'s.
    fn changed_rows(before: &[ObjectRecord], d: &Document) -> Vec<ObjectId> {
        let after = d.objects().expect("objects");
        assert_eq!(after.len(), before.len());
        after
            .iter()
            .filter(|o| before.iter().find(|b| b.id == o.id).expect("same rows") != *o)
            .map(|o| o.id)
            .collect()
    }

    /// §28I: under two sequential Fillets, adjacent and opposite, the base
    /// height alone changes — raised, lowered and kept — through the shipped
    /// preparation and writer; both Fillet rows, the Sketch, every name and
    /// every dependency stay; the preparation carries Fillet 1 as the Fillet
    /// on the base and Fillet 2 as rounding Fillet 1's result. Every other
    /// feature and bad height is refused before any writer.
    #[test]
    fn the_base_height_under_two_fillets_changes_alone_and_keeps_both() {
        for (r1, at, r2) in [(2.375, 2, 3.0625), (2.0, 3, 5.5)] {
            let (_root, mut d, first, second) = sequential(r1, at, r2);
            let base = base_of(&d, first);
            let sketch = match &d.object(base).expect("read").expect("base").payload {
                ObjectPayload::Extrude(e) => e.profile,
                _ => panic!("an Extrude"),
            };
            let refs = d.topology_refs().expect("refs");
            let deps = d.dependencies().expect("dependencies");
            for height in [11.5, 2.25, 2.25] {
                let objects = d.objects().expect("objects");
                let p = crate::prepare_extrude_height(&d, base, height).expect("prepared");
                let one = p.fillet().expect("Fillet 1");
                let two = p.fillets().get(1).expect("Fillet 2");
                assert_eq!((one.feature, one.previous), (first, base));
                assert_eq!(
                    (two.feature, two.previous),
                    (second, first),
                    "Fillet 2 rounds Fillet 1's result, not the base"
                );
                assert_eq!((one.history_index, two.history_index), (1, 2));
                assert!(p.history().is_none(), "not a Cut history");
                d.write_extrude_height(&p).expect("written");
                let moved = changed_rows(&objects, &d);
                let was = objects.iter().find(|o| o.id == base).expect("base");
                let same = match &was.payload {
                    ObjectPayload::Extrude(e) => matches!(
                        &e.end_condition,
                        EndCondition::Blind { distance } if distance.value() == height
                    ),
                    _ => false,
                };
                assert_eq!(moved, if same { vec![] } else { vec![base] });
                assert_eq!(stored_fillet(&d, first).radius_mm, r1);
                assert_eq!(stored_fillet(&d, second).radius_mm, r2);
                assert_eq!(stored_fillet(&d, second).previous, first);
                assert_eq!(d.topology_refs().expect("refs"), refs, "no name moved");
                assert_eq!(d.dependencies().expect("deps"), deps);
                assert!(d.validate().expect("validate").is_ok());
            }
            // Not the base: either Fillet or the Sketch; no bad height.
            for feature in [first, second, sketch, ObjectId::new()] {
                assert!(
                    crate::prepare_extrude_height(&d, feature, 9.5).is_err(),
                    "{feature}"
                );
            }
            for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
                assert_eq!(
                    crate::prepare_extrude_height(&d, base, bad)
                        .expect_err("bad height")
                        .kind(),
                    ErrorKind::Input
                );
            }
            // §28J/§28K: the Sketch and constraint editors read the same history.
            let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
            for s in &reading.sketches {
                assert_eq!(s.refusal, None);
                assert!(s.fillets.len() == 2);
            }
            for s in &reading.constraint_sketches {
                assert_eq!(s.refusal, None);
                assert!(s.fillets.len() == 2);
            }
        }
    }

    /// §28I: the height writer re-derives both Fillets. A preparation that
    /// drops or alters Fillet 2, swaps the two, names another row, or comes
    /// from another version, and a document changed after preparation, are
    /// refused and write nothing.
    #[test]
    fn the_height_writer_refuses_a_forged_two_fillet_preparation() {
        let (_root, mut d, first, second) = sequential(2.375, 2, 3.0625);
        let base = base_of(&d, first);
        let honest = crate::prepare_extrude_height(&d, base, 9.5).expect("prepared");
        let mut forged = Vec::new();
        let mut p = honest.clone();
        p.fillets.truncate(1);
        forged.push(("a preparation without Fillet 2", p));
        let mut p = honest.clone();
        if let Some(f) = p.fillets.get_mut(1) {
            f.radius_mm = 4.0;
        }
        forged.push(("another radius of Fillet 2", p));
        let mut p = honest.clone();
        if let Some(f) = p.fillets.get_mut(1) {
            f.previous = base;
        }
        forged.push(("Fillet 2 on the base", p));
        let mut p = honest.clone();
        p.fillets.swap(0, 1);
        forged.push(("the two swapped", p));
        let mut p = honest.clone();
        p.fillets.clear();
        forged.push(("no Fillet at all (the legacy writer)", p));
        let mut p = honest.clone();
        p.feature.id = second;
        forged.push(("Fillet 2's row", p));
        let mut p = honest.clone();
        p.source_version = ContentHash::of_bytes(b"another version");
        forged.push(("another version", p));
        let before = cells(&d);
        for (why, p) in &forged {
            assert!(d.write_extrude_height(p).is_err(), "{why} was written");
            assert_eq!(cells(&d), before, "{why} wrote something");
        }
        // Stale: Fillet 2's radius changed after the height was prepared.
        let radius = prepare_fillet_radius(&d, second, 2.5).expect("radius");
        d.write_fillet_radius(&radius).expect("radius written");
        let before = cells(&d);
        assert!(d.write_extrude_height(&honest).is_err(), "stale");
        assert_eq!(cells(&d), before, "stale wrote something");
    }

    /// §28H: the writer re-derives a two-Fillet radius edit; a forged payload,
    /// the other Fillet's row, a radius past the pair bound and a stale version
    /// are refused, writing nothing.
    #[test]
    fn the_writer_refuses_a_forged_sequential_radius_edit() {
        // r2 = 6.12: any r1 above 6.12 leaves less than 0.01 mm flat, within
        // §28A's 6.125 at each corner.
        let (_root, mut d, first, second) = sequential(2.375, 2, 6.12);
        let honest = prepare_fillet_radius(&d, first, 4.0).expect("prepared");
        let mut forged = Vec::new();
        let mut p = honest.clone();
        if let ObjectPayload::Fillet(f) = &mut p.feature.payload {
            f.radius_mm = 6.125;
        }
        forged.push(("a radius past the pair bound", p));
        let mut p = honest.clone();
        if let ObjectPayload::Fillet(f) = &mut p.feature.payload {
            f.previous = second;
        }
        forged.push(("another predecessor", p));
        let mut p = honest.clone();
        if let ObjectPayload::Fillet(f) = &mut p.feature.payload {
            f.edge = match &d.object(second).expect("read").expect("row").payload {
                ObjectPayload::Fillet(s) => s.edge,
                _ => unreachable!(),
            };
        }
        forged.push(("the other Fillet's corner", p));
        let mut p = honest.clone();
        p.feature.id = second;
        forged.push(("the other Fillet's row", p));
        let mut p = honest.clone();
        p.source_version = ContentHash::of_bytes(b"another version");
        forged.push(("another version", p));
        let before = cells(&d);
        for (why, p) in &forged {
            assert!(d.write_fillet_radius(p).is_err(), "{why} was written");
            assert_eq!(cells(&d), before, "{why} wrote something");
        }
        // Past the pair bound is refused before any writer, by the one rule.
        let e = prepare_fillet_radius(&d, first, 6.125).expect_err("pair");
        assert_eq!(e.kind(), ErrorKind::Input);
        assert!(e.to_string().contains("flat"), "{e}");
        d.write_fillet_radius(&honest).expect("the honest edit");
        assert_eq!(radius_of(&d, first), 4.0);
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
        let context = choice.fillet().cloned().expect("the Fillet as context");
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
        p.fillets.clear();
        forged.push(("a preparation without the Fillet", p));
        let mut p = honest.clone();
        if let Some(f) = p.fillets.get_mut(0) {
            f.radius_mm = 3.0;
        }
        forged.push(("another Fillet radius", p));
        let mut p = honest.clone();
        if let Some(f) = p.fillets.get_mut(0) {
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
        // The base Sketch is the Sketch edit's (§28D) and the constraint
        // editor's (§28E), with this Fillet as context.
        assert!(reading.sketches.iter().all(|s| s.fillet().is_some()));
        assert!(
            reading.constraint_sketches.iter().all(|s| {
                s.refusal.is_none() && s.fillet().is_some_and(|f| f.feature == fillet)
            })
        );
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
        assert!(row.fillets.is_empty() && row.refusal.is_some());

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

    /// §28D: the vertices of the base Sketch, in saved order, moved to `at`.
    fn starts(d: &Document, at: [[f64; 2]; 4]) -> (ObjectId, Vec<crate::SketchVertex>) {
        let reading = crate::ExtrudeEditSource::read(d).expect("catalogue");
        let choice = &reading.sketches[0];
        let vertices = choice.vertices.clone().expect("editable");
        (
            choice.sketch,
            vertices
                .into_iter()
                .zip(at)
                .map(|(v, start_mm)| crate::SketchVertex { start_mm, ..v })
                .collect(),
        )
    }

    fn corner_now(d: &Document) -> [f64; 2] {
        let reading = crate::ExtrudeEditSource::read(d).expect("catalogue");
        reading.sketches[0]
            .fillet()
            .expect("the Fillet as context")
            .corner
            .corner_mm
    }

    /// §28D: the base rectangle moves and resizes alone. Only the Sketch
    /// row's payload and hash move; the Fillet row, the Extrude, every name
    /// stay; the rounded corner follows its two Lines; Sketch, height and
    /// radius edits interleave on the same UUIDs.
    #[test]
    fn the_base_rectangle_moves_and_resizes_alone_and_interleaves() {
        let (_root, mut d, fillet) = filleted(2.375);
        let base = base_of(&d, fillet);
        let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        let choice = reading.sketches[0].clone();
        assert_eq!(choice.refusal, None);
        assert_eq!(choice.cut_history, None, "a Fillet is not a Cut history");
        let context = choice.fillet().cloned().expect("the Fillet as context");
        assert_eq!((context.feature, context.radius_mm), (fillet, 2.375));
        assert_eq!(context.corner.corner_mm, [33., 3.25]);
        let refs = d.topology_refs().expect("refs");
        let mut before = cells(&d);
        let mut radius = 2.375;
        // Each step: the new rectangle, and where the rounded corner must be.
        for (step, at) in [
            (
                "moved",
                [[-2., 1.75], [35.5, 1.75], [35.5, 14.], [-2., 14.]],
            ),
            (
                "larger",
                [
                    [-9.25, -2.5],
                    [41.75, -2.5],
                    [41.75, 17.125],
                    [-9.25, 17.125],
                ],
            ),
            // 2 × 2.375 = 4.75 is the narrowest depth the saved radius fits.
            (
                "narrowest",
                [[0.5, 1.], [20.25, 1.], [20.25, 5.75], [0.5, 5.75]],
            ),
            ("height", [[0.; 2]; 4]),
            ("radius", [[0.; 2]; 4]),
            (
                "after both",
                [[-4.5, 3.25], [33., 3.25], [33., 15.5], [-4.5, 15.5]],
            ),
        ] {
            let row = match step {
                "height" => {
                    let p = crate::prepare_extrude_height(&d, base, 9.5).expect("height");
                    d.write_extrude_height(&p).expect("height written");
                    base
                }
                "radius" => {
                    radius = 1.1875;
                    let p = prepare_fillet_radius(&d, fillet, radius).expect("radius");
                    d.write_fillet_radius(&p).expect("radius written");
                    fillet
                }
                _ => {
                    let (sketch, vertices) = starts(&d, at);
                    let prepared =
                        crate::replace_sketch_coordinates(&d, sketch, &vertices).expect(step);
                    d.write_sketch_geometry(&prepared).expect(step);
                    assert_eq!(corner_now(&d), at[1], "{step}: the same corner, moved");
                    sketch
                }
            };
            let after = cells(&d);
            assert!(only_this_row_moved(&before, &after, row) >= 2, "{step}");
            before = after;
            let stored = stored_fillet(&d, fillet);
            assert_eq!(stored.edge, context.edge, "{step}");
            assert_eq!(stored.previous, base, "{step}");
            assert_eq!(stored.radius_mm, radius, "{step}");
            assert_eq!(
                d.topology_refs().expect("refs"),
                refs,
                "{step}: no name moved"
            );
            assert!(d.validate().expect("validate").is_ok(), "{step}");
        }
        assert_eq!(height_of(&d, base), 9.5, "the Sketch edit kept the height");
    }

    /// §28D refusals, each by the domain check the form and the writer share:
    /// a rectangle too small for the saved radius, a Line that changes side,
    /// a shape that is no longer a rectangle, the loop in another order.
    #[test]
    fn a_candidate_rectangle_that_does_not_keep_the_rounded_corner_is_refused() {
        let (_root, mut d, _) = filleted(2.375);
        let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        let choice = &reading.sketches[0];
        let (_, vertices) = starts(&d, [[0.; 2]; 4]);
        let with = |at: [[f64; 2]; 4]| -> Vec<crate::SketchVertex> {
            vertices
                .iter()
                .zip(at)
                .map(|(v, start_mm)| crate::SketchVertex {
                    start_mm,
                    ..v.clone()
                })
                .collect()
        };
        for (why, at, says) in [
            (
                "too shallow for r",
                [[0.5, 1.], [20.25, 1.], [20.25, 5.74], [0.5, 5.74]],
                "too large",
            ),
            (
                "too narrow for r",
                [[0., 0.], [4.7, 0.], [4.7, 9.], [0., 9.]],
                "too large",
            ),
            (
                "Lines rotated to other sides",
                [[0., 20.], [0., 0.], [40., 0.], [40., 20.]],
                "keep its side",
            ),
            (
                "a trapezoid",
                [[0., 0.], [40., 0.], [35., 12.], [5., 12.]],
                "axis-aligned rectangle",
            ),
        ] {
            let error = choice.validate_coordinates(&with(at)).expect_err(why);
            assert!(error.to_string().contains(says), "{why}: {error}");
        }
        let mut reordered = with([[0., 0.], [40., 0.], [40., 20.], [0., 20.]]);
        reordered.swap(1, 3);
        assert!(
            choice.validate_coordinates(&reordered).is_err(),
            "reordered"
        );
        // The rectangle reader accepts sub-tolerance coordinate noise. It
        // does not turn a horizontal side into a different side, in either
        // the candidate or the saved profile. Keep the supplied numbers;
        // there is no snapping in a coordinate edit.
        let clean = [[0., 0.], [40., 0.], [40., 20.], [0., 20.]];
        let mut noisy = clean;
        noisy[1][1] = 1e-10;
        let p = crate::replace_sketch_coordinates(&d, choice.sketch, &with(noisy))
            .expect("same side within the rectangle reader's tolerance");
        d.write_sketch_geometry(&p).expect("write noisy rectangle");
        let p = crate::replace_sketch_coordinates(&d, choice.sketch, &with(clean))
            .expect("a saved nearly horizontal side can become exactly horizontal");
        d.write_sketch_geometry(&p).expect("write exact rectangle");
    }

    /// §28D: the coordinate writer re-derives the edit from the new
    /// coordinates against the Fillet as it is now. A forged payload, a
    /// Sketch changed after preparation, and a Fillet whose radius no longer
    /// fits the prepared rectangle are all refused and write nothing.
    #[test]
    fn the_sketch_writer_refuses_a_forged_or_stale_preparation() {
        let (_root, mut d, fillet) = filleted(2.375);
        let (sketch, vertices) = starts(&d, [[0., 0.], [20., 0.], [20., 6.], [0., 6.]]);
        let honest = crate::replace_sketch_coordinates(&d, sketch, &vertices).expect("prepared");
        let forge = |edit: &dyn Fn(&mut crate::Sketch)| {
            let mut p = honest.clone();
            let ObjectPayload::Sketch(s) = &mut p.payload else {
                panic!("a Sketch")
            };
            edit(s);
            p
        };
        let line = |s: &mut crate::Sketch, i: usize, a: [f64; 2], b: [f64; 2]| {
            s.curves[i].geometry = SketchGeometry::Line {
                start: Point2::new(a[0], a[1]).expect("a"),
                end: Point2::new(b[0], b[1]).expect("b"),
            };
        };
        let forged = [
            (
                "too small for r",
                forge(&|s| {
                    line(s, 1, [20., 0.], [20., 4.]);
                    line(s, 2, [20., 4.], [0., 4.]);
                    line(s, 3, [0., 4.], [0., 0.]);
                }),
            ),
            (
                "another plane",
                forge(&|s| {
                    s.plane = ObjectId::new();
                }),
            ),
            (
                "a construction Line",
                forge(&|s| {
                    s.curves[0].construction = true;
                }),
            ),
            ("the loop reversed", forge(&|s| s.curves.reverse())),
        ];
        let before = cells(&d);
        for (why, p) in &forged {
            assert!(d.write_sketch_geometry(p).is_err(), "{why} was written");
            assert_eq!(cells(&d), before, "{why} wrote something");
        }
        // Stale: the Fillet's radius grows after preparation to one the
        // prepared 6 mm deep rectangle cannot hold.
        let p = prepare_fillet_radius(&d, fillet, 3.5).expect("still fits the saved plate");
        d.write_fillet_radius(&p).expect("radius written");
        let before = cells(&d);
        let error = d.write_sketch_geometry(&honest).expect_err("stale");
        assert!(error.to_string().contains("too large"), "{error}");
        assert_eq!(cells(&d), before, "stale wrote something");
    }

    /// §28D: a Fillet outside the frame keeps every Sketch refused, naming
    /// the Fillet and the reason; the constraint editor refuses regardless.
    #[test]
    fn a_sketch_under_a_fillet_outside_the_frame_is_refused_with_its_reason() {
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
        let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        let row = &reading.sketches[0];
        assert!(row.vertices.is_none() && row.fillets.is_empty());
        let reason = row.refusal.as_deref().expect("refused");
        assert!(
            reason.contains(&fillet.to_string()) && reason.contains("more faces"),
            "{reason}"
        );
        let (sketch, _) = (row.sketch, ());
        let vertices = vec![];
        assert!(crate::replace_sketch_coordinates(&d, sketch, &vertices).is_err());
    }

    /// The plate's Sketch, its Line UUIDs in stored order.
    fn plate_lines(d: &Document) -> (ObjectId, Vec<StableEntityId>) {
        let objects = d.objects().expect("objects");
        let sketch = objects
            .iter()
            .find(|o| matches!(o.payload, ObjectPayload::Sketch(_)))
            .expect("Sketch");
        let ObjectPayload::Sketch(s) = &sketch.payload else {
            unreachable!()
        };
        (sketch.id, s.curves.iter().map(|c| c.id).collect())
    }

    fn line_rule(
        curve: StableEntityId,
        kind: crate::LineConstraintKind,
    ) -> crate::AddSketchConstraint {
        crate::AddSketchConstraint::Line(crate::AddLineConstraint::Line { curve, kind })
    }

    /// §28E: H on the first Line, through the shipped preparation and writer.
    fn constrained(d: &mut Document) -> crate::PreparedSketchConstraints {
        let (sketch, lines) = plate_lines(d);
        let edits = crate::SketchConstraintEdits {
            remove: Vec::new(),
            add: vec![line_rule(lines[0], crate::LineConstraintKind::Horizontal)],
        };
        let prepared = crate::prepare_sketch_constraints(d, sketch, &edits).expect("prepared");
        d.write_sketch_constraints(&prepared).expect("written");
        prepared
    }

    /// Four Lines through `at`, under the plate's own UUIDs, as a rebuild
    /// would present them.
    fn built(lines: &[StableEntityId], at: [[f64; 2]; 4]) -> Vec<SketchCurve> {
        (0..4)
            .map(|i| SketchCurve {
                id: lines[i],
                construction: false,
                geometry: SketchGeometry::Line {
                    start: Point2::new(at[i][0], at[i][1]).expect("point"),
                    end: Point2::new(at[(i + 1) % 4][0], at[(i + 1) % 4][1]).expect("point"),
                },
            })
            .collect()
    }

    /// §28E: constraints on the rounded plate's base. Only the Sketch row's
    /// schema version, payload and hash, and the constraint capability row,
    /// move. The stored corner is reported as it is stored; the radius bound
    /// is not read off the stored lengths in either direction; the height
    /// still edits and keeps the constraints; the coordinate editor refuses a
    /// constrained Sketch as it always has.
    #[test]
    fn constraints_on_the_rounded_plate_change_only_the_sketch_and_defer_the_bound() {
        let (_root, mut d, fillet) = filleted(2.375);
        let (sketch, _) = plate_lines(&d);
        let before = cells(&d);
        let refs = d.topology_refs().expect("refs");
        constrained(&mut d);
        let after = cells(&d);
        let selected = rusqlite::types::Value::Blob(sketch.to_bytes().to_vec());
        for (table, (columns, rows)) in &before {
            let (_, mine) = &after[table];
            if table == "capabilities" {
                assert!(
                    rows.iter().all(|r| mine.contains(r)),
                    "a capability changed"
                );
                let new: Vec<_> = mine.iter().filter(|r| !rows.contains(r)).collect();
                assert_eq!(new.len(), 1, "{new:?}");
                assert!(format!("{new:?}").contains("sketch.constraints.v1"));
                continue;
            }
            assert_eq!(rows.len(), mine.len(), "{table}");
            for (a, b) in rows.iter().zip(mine) {
                for (i, column) in columns.iter().enumerate() {
                    if a[i] != b[i] {
                        assert!(
                            table == "objects"
                                && ["schema_version", "payload", "payload_hash"]
                                    .contains(&column.as_str())
                                && a.contains(&selected),
                            "{table}.{column} changed"
                        );
                    }
                }
            }
        }
        assert_eq!(d.topology_refs().expect("refs"), refs);

        let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        let choice = &reading.constraint_sketches[0];
        let saved = choice.fillet().expect("the Fillet as context");
        assert!(saved.constrained);
        assert_eq!(saved.feature, fillet);
        assert_eq!(saved.corner.corner_mm, [33., 3.25], "the stored corner");
        // 50 mm is far beyond the stored bound of 6.125 mm; whether it fits is
        // the solved plate's question. The corner-free part still holds.
        saved
            .check_radius(50.)
            .expect("the bound is the solved plate's");
        assert!(saved.check_radius(0.001).is_err());
        assert!(saved.check_radius(f64::NAN).is_err());
        let radius = prepare_fillet_radius(&d, fillet, 50.).expect("deferred to the rebuild");
        assert!(radius.saved().constrained);
        assert!(
            reading.sketches[0]
                .refusal
                .as_deref()
                .is_some_and(|r| r.contains("unconstrained")),
            "{:?}",
            reading.sketches[0].refusal
        );
        // The height keeps the constraints byte for byte.
        let stored = d.object(sketch).expect("read").expect("Sketch");
        let base = base_of(&d, fillet);
        let height = crate::prepare_extrude_height(&d, base, 9.5).expect("height");
        d.write_extrude_height(&height).expect("written");
        let kept = d.object(sketch).expect("read").expect("Sketch");
        assert_eq!(kept.storage_bytes(), stored.storage_bytes());
        // Creating a Fillet still requires an unconstrained profile (§28A).
        let objects = d.objects().expect("objects");
        let ObjectPayload::Sketch(s) = &stored.payload else {
            unreachable!()
        };
        assert!(crate::rectangle_corners(base, s).is_err());
        drop(objects);
    }

    /// §28E: the constraint writer derives the edit again inside its
    /// transaction. A payload that moves a curve, drops a closure link, adds a
    /// constraint the preparation did not, or was prepared before the Fillet
    /// changed, is refused and writes nothing.
    #[test]
    fn the_constraint_writer_rederives_the_edit_under_a_fillet() {
        let (_root, mut d, fillet) = filleted(2.375);
        constrained(&mut d);
        let (sketch, lines) = plate_lines(&d);
        let edits = crate::SketchConstraintEdits {
            remove: Vec::new(),
            add: vec![line_rule(lines[1], crate::LineConstraintKind::Vertical)],
        };
        let good = crate::prepare_sketch_constraints(&d, sketch, &edits).expect("prepared");
        let untouched = cells(&d);
        let refuse = |d: &mut Document, p: &crate::PreparedSketchConstraints, why: &str| {
            let e = d.write_sketch_constraints(p).expect_err(why);
            assert_eq!(e.kind(), ErrorKind::Input, "{why}: {e}");
            assert_eq!(cells(d), untouched, "{why} wrote something");
            e
        };
        // A moved curve.
        let mut moved = good.clone();
        let ObjectPayload::Sketch(s) = &mut moved.object.payload else {
            unreachable!()
        };
        s.curves = built(
            &lines,
            [[-4.5, 3.25], [40., 3.25], [40., 15.5], [-4.5, 15.5]],
        );
        refuse(&mut d, &moved, "moved curves");
        // A closure link dropped from the payload and named as removed.
        let mut dropped = good.clone();
        let ObjectPayload::Sketch(s) = &mut dropped.object.payload else {
            unreachable!()
        };
        let link = s.constraints[0];
        s.constraints.remove(0);
        dropped.removed = vec![link.id];
        let e = refuse(&mut d, &dropped, "closure");
        assert!(e.to_string().contains("closure"), "{e}");
        // An extra constraint the preparation did not add.
        let mut extra = good.clone();
        let ObjectPayload::Sketch(s) = &mut extra.object.payload else {
            unreachable!()
        };
        s.constraints.push(crate::SketchConstraint {
            id: StableEntityId::new(),
            rule: crate::SketchConstraintRule::Horizontal {
                a: crate::SketchPointRef::new(lines[2], crate::SketchPointSelector::Start),
                b: crate::SketchPointRef::new(lines[2], crate::SketchPointSelector::End),
            },
        });
        refuse(&mut d, &extra, "extra");
        // Stale: the Fillet's radius changed after preparation.
        let radius = prepare_fillet_radius(&d, fillet, 3.).expect("radius");
        d.write_fillet_radius(&radius).expect("written");
        let changed = cells(&d);
        let e = d.write_sketch_constraints(&good).expect_err("stale");
        assert_eq!(e.kind(), ErrorKind::Input, "{e}");
        assert!(e.to_string().contains("Fillet"), "{e}");
        assert_eq!(cells(&d), changed);
    }

    /// §28E: the evaluator's question, asked of the Lines the plate was built
    /// from. For a constrained plate those are the solved ones, and the stored
    /// numbers prove nothing: the bound, the sides, the class and the corner
    /// are all judged on `built`, on both sides of each boundary.
    #[test]
    fn the_evaluator_judges_the_fillet_on_the_built_lines() {
        let (_root, mut d, fillet) = filleted(2.375);
        let (_, lines) = plate_lines(&d);
        let payload = stored_fillet(&d, fillet);
        let objects = d.objects().expect("objects");
        let saved = [PLATE[0], PLATE[1], PLATE[2], PLATE[3]];
        // Unconstrained: exactly the stored Lines, as before.
        crate::evaluable_fillet(&objects, &payload, Some(&built(&lines, saved)))
            .expect("the stored plate");
        assert!(crate::evaluable_fillet(&objects, &payload, None).is_err());
        drop(objects);

        constrained(&mut d);
        let objects = d.objects().expect("objects");
        let judge = |at: [[f64; 2]; 4]| {
            crate::evaluable_fillet(&objects, &payload, Some(&built(&lines, at)))
        };
        let rect = |x0: f64, y0: f64, w: f64, h: f64| {
            [[x0, y0], [x0 + w, y0], [x0 + w, y0 + h], [x0, y0 + h]]
        };
        // Solved larger or smaller than stored: the corner follows its two
        // Lines, and the bound is the solved one, exactly at 2 r.
        let corner = judge(rect(1., 2., 30., 4.75)).expect("exactly 2 r deep");
        assert_eq!(corner.corner_mm, [31., 2.]);
        assert_eq!(corner.adjacent_lengths_mm, [30., 4.75]);
        let e = judge(rect(1., 2., 30., 4.749_999)).expect_err("just under 2 r");
        assert_eq!(e.kind(), ErrorKind::Input);
        assert!(e.to_string().contains("too short"), "{e}");
        // The shared rectangle reader's tolerance, 1e-7 mm, on both sides.
        let mut near = rect(1., 2., 30., 10.);
        near[1][1] += 0.9e-7;
        judge(near).expect("within the rectangle reader's tolerance");
        let mut off = rect(1., 2., 30., 10.);
        off[1][1] += 1.1e-7;
        let e = judge(off).expect_err("beyond it");
        assert_eq!(e.kind(), ErrorKind::Unsupported);
        assert!(e.to_string().contains("rectangle"), "{e}");
        // A Line reversed: the first Line runs -X; still a rectangle.
        let e = judge([[31., 2.], [26., 2.], [26., 12.], [31., 12.]]).expect_err("reversed");
        assert_eq!(e.kind(), ErrorKind::Input);
        assert!(e.to_string().contains("side"), "{e}");
        // The same four Lines in another order.
        let mut shuffled = built(&lines, rect(1., 2., 30., 10.));
        shuffled.rotate_left(1);
        let e =
            crate::evaluable_fillet(&objects, &payload, Some(&shuffled)).expect_err("reordered");
        assert!(e.to_string().contains("same four Lines"), "{e}");
    }

    /// §28J: a rectangle by its lower-left and upper-right corner, in stored
    /// order: counter-clockwise, or clockwise starting at the upper left.
    fn rect(cw: bool, (x0, y0): (f64, f64), (x1, y1): (f64, f64)) -> [[f64; 2]; 4] {
        if cw {
            [[x0, y1], [x1, y1], [x1, y0], [x0, y0]]
        } else {
            [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
        }
    }

    /// §28J: the two Fillets as the Sketch editor reads them.
    fn both(d: &Document) -> (crate::SketchChoice, crate::SavedFillet, crate::SavedFillet) {
        let reading = crate::ExtrudeEditSource::read(d).expect("catalogue");
        let choice = reading.sketches[0].clone();
        let one = choice.fillet().cloned().expect("Fillet 1 as context");
        let two = choice.fillets.get(1).cloned().expect("Fillet 2 as context");
        (choice, one, two)
    }

    /// §28J: under two Fillets, adjacent and opposite, in both windings, the
    /// base rectangle moves, grows and shrinks to the exact bound alone. Only
    /// the Sketch row's payload, hash and the stamp move; both Fillet rows,
    /// the Extrude, every name and dependency stay; each rounded corner
    /// follows its own two Lines; height and either radius interleave.
    #[test]
    fn the_rectangle_under_two_fillets_moves_alone_and_both_corners_follow() {
        // (clockwise, r1, second corner, r2, the narrowest side both allow)
        for (cw, r1, at, r2, narrowest) in [
            (false, 2.375, 2, 3.0625, 6.125),
            (true, 2.375, 2, 3.0625, 6.125),
            (false, 2.0, 3, 5.5, 11.0),
        ] {
            let plate = rect(cw, (-4.5, 3.25), (33., 15.5));
            let (_root, mut d, first, second) = sequential_on(plate, r1, at, r2);
            let base = base_of(&d, first);
            let (choice, one, two) = both(&d);
            assert_eq!(choice.refusal, None);
            assert_eq!((one.feature, one.previous), (first, base));
            assert_eq!(
                (two.feature, two.previous, two.base_feature),
                (second, first, base),
                "Fillet 2 rounds Fillet 1's result; both edges are the base's"
            );
            let refs = d.topology_refs().expect("refs");
            let deps = d.dependencies().expect("deps");
            let mut before = cells(&d);
            let steps = [
                ("moved", rect(cw, (-2., 1.75), (35.5, 14.))),
                ("larger", rect(cw, (-9.25, -2.5), (41.75, 17.125))),
                (
                    "narrowest",
                    rect(cw, (0.5, 1.), (0.5 + 3. * narrowest, 1. + narrowest)),
                ),
                ("restored", plate),
            ];
            for (step, at_step) in steps {
                let (sketch, vertices) = starts(&d, at_step);
                let prepared =
                    crate::replace_sketch_coordinates(&d, sketch, &vertices).expect(step);
                d.write_sketch_geometry(&prepared).expect(step);
                let (_, one_now, two_now) = both(&d);
                assert_eq!(one_now.corner.corner_mm, at_step[1], "{step}: Fillet 1");
                assert_eq!(two_now.corner.corner_mm, at_step[at], "{step}: Fillet 2");
                let after = cells(&d);
                assert!(only_this_row_moved(&before, &after, sketch) >= 2, "{step}");
                before = after;
                for (id, r, previous) in [(first, r1, base), (second, r2, first)] {
                    let stored = stored_fillet(&d, id);
                    assert_eq!((stored.radius_mm, stored.previous), (r, previous), "{step}");
                }
                assert_eq!(d.topology_refs().expect("refs"), refs, "{step}: no name");
                assert_eq!(d.dependencies().expect("deps"), deps, "{step}");
                assert!(d.validate().expect("validate").is_ok(), "{step}");
            }
            // Height and each radius still edit, and the Sketch again after.
            let p = crate::prepare_extrude_height(&d, base, 9.5).expect("height");
            d.write_extrude_height(&p).expect("height written");
            for (id, r) in [(first, r1 * 0.5), (second, r2 * 0.5)] {
                let p = prepare_fillet_radius(&d, id, r).expect("radius");
                d.write_fillet_radius(&p).expect("radius written");
            }
            let (sketch, vertices) = starts(&d, rect(cw, (1., 1.), (60., 40.)));
            let p = crate::replace_sketch_coordinates(&d, sketch, &vertices).expect("again");
            d.write_sketch_geometry(&p).expect("again written");
            assert_eq!(height_of(&d, base), 9.5);
        }
    }

    /// §28J refusals, each by the check the form and the writer share, naming
    /// the Fillet that fails: a Line that changes side, a shape that is no
    /// longer a rectangle, a side too short for Fillet 1's radius or for
    /// Fillet 2's alone, and the flat between two adjacent arcs — at exactly
    /// the bound and one float below it — which opposite corners never owe.
    #[test]
    fn a_rectangle_that_loses_a_corner_a_radius_or_the_shared_flat_is_refused() {
        let ask = |d: &Document, at: [[f64; 2]; 4]| {
            let (choice, ..) = both(d);
            let (_, vertices) = starts(d, at);
            choice.validate_coordinates(&vertices)
        };
        // The wider Fillet is the second: its bound is what fails first.
        let (_a, d, first, second) = sequential(2.0, 3, 5.5);
        let (_, one, two) = both(&d);
        assert_eq!((one.feature, two.feature), (first, second));
        ask(&d, rect(false, (0., 0.), (30., 11.))).expect("exactly 2 r2");
        let e = ask(&d, rect(false, (0., 0.), (30., 10.9))).expect_err("under 2 r2");
        assert_eq!(e.kind(), ErrorKind::Input);
        assert!(e.to_string().contains(&two.edge.joint.to_string()), "{e}");
        assert!(!e.to_string().contains(&one.edge.joint.to_string()), "{e}");
        // The wider Fillet is the first: Fillet 1's own bound names Fillet 1.
        let (_b, d, ..) = sequential(5.5, 3, 2.0);
        let (_, one, two) = both(&d);
        let e = ask(&d, rect(false, (0., 0.), (30., 10.9))).expect_err("under 2 r1");
        assert!(e.to_string().contains(&one.edge.joint.to_string()), "{e}");
        assert!(!e.to_string().contains(&two.edge.joint.to_string()), "{e}");
        // The same Lines in another order, and a rectangle turned to a skew.
        let plate = rect(false, (-4.5, 3.25), (33., 15.5));
        let mut shifted = plate;
        shifted.rotate_left(1);
        let e = ask(&d, shifted).expect_err("Lines swap their sides");
        assert!(e.to_string().contains("keep its side"), "{e}");
        let e = ask(&d, [[0., 0.], [20., 0.], [20., 12.], [1., 12.]]).expect_err("not a rectangle");
        assert_eq!(e.kind(), ErrorKind::Unsupported);
        // The pair: r1 = r2 = 3 on adjacent corners share the Line x = 33.
        // Each radius alone fits a 6.005 mm deep plate; the flat between them
        // does not, and the bound is the predicate itself, not an epsilon.
        let (_c, adjacent, ..) = sequential(3.0, 2, 3.0);
        let deep = |d: f64| rect(false, (0., 0.), (20., d));
        let e = ask(&adjacent, deep(6.005)).expect_err("no flat left");
        assert_eq!(e.kind(), ErrorKind::Input);
        assert!(e.to_string().contains("flat"), "{e}");
        let mut fits = 6.01_f64;
        while 3.0 > crate::fillet::pair_bound(fits, 3.0) {
            fits = fits.next_up();
        }
        ask(&adjacent, deep(fits)).expect("the least depth that leaves the flat");
        let e = ask(&adjacent, deep(fits.next_down())).expect_err("one float below");
        assert!(e.to_string().contains("flat"), "{e}");
        // Opposite corners share no Line: only each radius counts (2 r = 6).
        let (_o, opposite, ..) = sequential(3.0, 3, 3.0);
        ask(&opposite, deep(6.0)).expect("no shared Line, no flat owed");
        let e = ask(&opposite, deep(5.999)).expect_err("under 2 r");
        assert!(e.to_string().contains("too large"), "{e}");
        // Nothing was written by any refusal.
        let before = cells(&adjacent);
        let (sketch, vertices) = starts(&adjacent, deep(6.005));
        assert!(crate::replace_sketch_coordinates(&adjacent, sketch, &vertices).is_err());
        assert_eq!(cells(&adjacent), before);
    }

    /// §28J: the writer re-derives both Fillets. A preparation that no longer
    /// fits Fillet 2, a Sketch changed after preparation, a Fillet 2 radius
    /// that grew past the prepared rectangle, and a forged payload are all
    /// refused, writing nothing.
    #[test]
    fn the_sketch_writer_rederives_both_fillets_and_refuses_forgery() {
        let (_root, mut d, first, second) = sequential(2.375, 2, 3.0625);
        let (sketch, vertices) = starts(&d, rect(false, (0., 0.), (20., 6.125)));
        let honest = crate::replace_sketch_coordinates(&d, sketch, &vertices).expect("prepared");
        let forge = |edit: &dyn Fn(&mut crate::Sketch)| {
            let mut p = honest.clone();
            let ObjectPayload::Sketch(s) = &mut p.payload else {
                panic!("a Sketch")
            };
            edit(s);
            p
        };
        let line = |s: &mut crate::Sketch, i: usize, a: [f64; 2], b: [f64; 2]| {
            s.curves[i].geometry = SketchGeometry::Line {
                start: Point2::new(a[0], a[1]).expect("a"),
                end: Point2::new(b[0], b[1]).expect("b"),
            };
        };
        let forged = [
            (
                "5.9 mm deep: under 2 r2 (Fillet 2)",
                forge(&|s| {
                    line(s, 1, [20., 0.], [20., 5.9]);
                    line(s, 2, [20., 5.9], [0., 5.9]);
                    line(s, 3, [0., 5.9], [0., 0.]);
                }),
            ),
            (
                "another plane",
                forge(&|s| {
                    s.plane = ObjectId::new();
                }),
            ),
            ("the loop reversed", forge(&|s| s.curves.reverse())),
            (
                "a constraint that was never there",
                forge(&|s| {
                    s.constraints.push(crate::SketchConstraint {
                        id: StableEntityId::new(),
                        rule: crate::SketchConstraintRule::Horizontal {
                            a: crate::SketchPointRef::new(
                                s.curves[0].id,
                                crate::SketchPointSelector::Start,
                            ),
                            b: crate::SketchPointRef::new(
                                s.curves[0].id,
                                crate::SketchPointSelector::End,
                            ),
                        },
                    });
                }),
            ),
        ];
        let before = cells(&d);
        for (why, p) in &forged {
            assert!(d.write_sketch_geometry(p).is_err(), "{why} was written");
            assert_eq!(cells(&d), before, "{why} wrote something");
        }
        // Stale: Fillet 2 grows to 3.0625 → 3.5 after preparation. The
        // prepared 6.125 mm deep rectangle cannot hold it (2 × 3.5 = 7).
        let p = prepare_fillet_radius(&d, second, 3.5).expect("fits the saved plate");
        d.write_fillet_radius(&p).expect("radius written");
        let before = cells(&d);
        let error = d.write_sketch_geometry(&honest).expect_err("stale");
        assert!(error.to_string().contains("too large"), "{error}");
        assert_eq!(cells(&d), before, "stale wrote something");
        // Fillet 1 keeps its radius throughout.
        assert_eq!(stored_fillet(&d, first).radius_mm, 2.375);
    }

    /// §28J: what the Sketch may carry under two Fillets. Only the Coincident
    /// closure links §28E leaves are kept — byte for byte — and edited
    /// around; any other constraint refuses the coordinate editor, naming the
    /// constraint editor (§28K) that edits it.
    #[test]
    fn under_two_fillets_only_closure_links_survive_a_coordinate_edit() {
        let (_root, mut d, ..) = sequential(2.375, 2, 3.0625);
        let (sketch, lines) = plate_lines(&d);
        let object = d.object(sketch).expect("read").expect("Sketch");
        let mut payload = object.payload.clone();
        let ObjectPayload::Sketch(s) = &mut payload else {
            unreachable!()
        };
        let point = |curve, at| crate::SketchPointRef::new(curve, at);
        let joins: Vec<_> = (0..4)
            .map(|i| crate::SketchConstraint {
                id: StableEntityId::new(),
                rule: crate::SketchConstraintRule::Coincident {
                    a: point(lines[i], crate::SketchPointSelector::End),
                    b: point(lines[(i + 1) % 4], crate::SketchPointSelector::Start),
                },
            })
            .collect();
        s.constraints = joins.clone();
        let put = |d: &mut Document, payload: &ObjectPayload| {
            d.write(|w| {
                w.put_object(
                    sketch,
                    None,
                    object.ordinal,
                    object.name.as_deref(),
                    payload,
                )
            })
            .expect("stored");
        };
        put(&mut d, &payload);
        let (choice, ..) = both(&d);
        assert_eq!(choice.refusal, None, "closure links are free");
        let (_, vertices) = starts(&d, rect(false, (0., 0.), (20., 7.)));
        let prepared = crate::replace_sketch_coordinates(&d, sketch, &vertices).expect("edit");
        d.write_sketch_geometry(&prepared).expect("written");
        let ObjectPayload::Sketch(now) = d.object(sketch).expect("read").expect("row").payload
        else {
            unreachable!()
        };
        assert_eq!(now.constraints, joins, "the links are kept byte for byte");
        assert!(d.validate().expect("validate").is_ok());
        // One more constraint and the coordinate editor refuses, naming why.
        let ObjectPayload::Sketch(s) = &mut payload else {
            unreachable!()
        };
        s.constraints.push(crate::SketchConstraint {
            id: StableEntityId::new(),
            rule: crate::SketchConstraintRule::Horizontal {
                a: point(lines[0], crate::SketchPointSelector::Start),
                b: point(lines[0], crate::SketchPointSelector::End),
            },
        });
        put(&mut d, &payload);
        let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
        let row = &reading.sketches[0];
        assert!(row.vertices.is_none() && row.fillets.get(1).is_none());
        let reason = row.refusal.as_deref().expect("refused");
        assert!(reason.contains("edit-sketch-constraints-copy"), "{reason}");
        let before = cells(&d);
        assert!(crate::replace_sketch_coordinates(&d, sketch, &vertices).is_err());
        assert_eq!(cells(&d), before);
    }

    /// §28K: every cell but the base Sketch row's schema version, payload and
    /// hash, the stamp and — once, when the Sketch first carries constraints —
    /// the `sketch.constraints.v1` capability row, is the same.
    fn only_the_constraints_moved(
        before: &std::collections::BTreeMap<String, (Vec<String>, Rows)>,
        after: &std::collections::BTreeMap<String, (Vec<String>, Rows)>,
        sketch: ObjectId,
        new_capabilities: usize,
    ) {
        let selected = rusqlite::types::Value::Blob(sketch.to_bytes().to_vec());
        for (table, (columns, rows)) in before {
            let (_, mine) = &after[table];
            if table == "capabilities" {
                assert!(
                    rows.iter().all(|r| mine.contains(r)),
                    "a capability changed"
                );
                let new: Vec<_> = mine.iter().filter(|r| !rows.contains(r)).collect();
                assert_eq!(new.len(), new_capabilities, "{new:?}");
                continue;
            }
            assert_eq!(rows.len(), mine.len(), "{table}");
            for (a, b) in rows.iter().zip(mine) {
                for (i, column) in columns.iter().enumerate() {
                    if a[i] != b[i] {
                        assert!(
                            (table == "objects"
                                && ["schema_version", "payload", "payload_hash"]
                                    .contains(&column.as_str())
                                && a.contains(&selected))
                                || (table == "meta" && column == "modified_at"),
                            "{table}.{column} changed"
                        );
                    }
                }
            }
        }
    }

    fn stored_constraints(d: &Document, sketch: ObjectId) -> Vec<crate::SketchConstraint> {
        match d.object(sketch).expect("read").expect("Sketch").payload {
            ObjectPayload::Sketch(s) => s.constraints,
            _ => panic!("a Sketch"),
        }
    }

    /// §28K: under two Fillets, adjacent and opposite, in both windings, the
    /// constraint editor adds and removes Line constraints through the shipped
    /// preparation and writer. Only the base Sketch row (its payload, hash and
    /// schema version) and the one capability row move; both Fillet rows, the
    /// Extrude, every name and dependency stay; the closure links keep their
    /// UUIDs through a removal; the catalogue names both Fillets in history
    /// order and, once only the closure links remain, offers the Sketch's
    /// coordinates (§28J) again.
    #[test]
    fn constraints_under_two_fillets_change_only_the_sketch_and_keep_both_fillets() {
        for (cw, r1, at, r2) in [(false, 2.375, 2, 3.0625), (true, 2.0, 3, 5.5)] {
            let plate = rect(cw, (-4.5, 3.25), (33., 15.5));
            let (_root, mut d, first, second) = sequential_on(plate, r1, at, r2);
            let base = base_of(&d, first);
            let (sketch, lines) = plate_lines(&d);
            let refs = d.topology_refs().expect("refs");
            let deps = d.dependencies().expect("deps");
            let rows = |d: &Document| {
                (
                    d.object(first).expect("read").expect("Fillet 1"),
                    d.object(second).expect("read").expect("Fillet 2"),
                    d.object(base).expect("read").expect("Extrude"),
                )
            };
            let (f1, f2, extrude) = rows(&d);
            let before = cells(&d);
            constrained(&mut d);
            let after = cells(&d);
            only_the_constraints_moved(&before, &after, sketch, 1);
            assert_eq!(
                rows(&d),
                (f1.clone(), f2.clone(), extrude.clone()),
                "unchanged rows"
            );
            assert_eq!(d.topology_refs().expect("refs"), refs);
            assert_eq!(d.dependencies().expect("deps"), deps);
            assert!(d.validate().expect("validate").is_ok());

            let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
            let choice = &reading.constraint_sketches[0];
            assert_eq!(choice.refusal, None, "{choice:?}");
            let (one, two) = (
                choice.fillet().expect("Fillet 1"),
                choice.fillets.get(1).expect("Fillet 2"),
            );
            assert_eq!((one.feature, one.previous), (first, base));
            assert_eq!(
                (
                    two.feature,
                    two.previous,
                    two.base_feature,
                    two.history_index
                ),
                (second, first, base, 2),
                "Fillet 2 rounds Fillet 1's result; its edge is the base's"
            );
            assert!(one.constrained && two.constrained);
            // The stored numbers prove nothing about the solved part.
            one.check_radius(50.)
                .expect("the bound is the solved plate's");
            two.check_radius(50.)
                .expect("the bound is the solved plate's");
            assert!(two.check_radius(f64::NAN).is_err());
            assert!(
                reading.sketches[0]
                    .refusal
                    .as_deref()
                    .is_some_and(|r| r.contains("edit-sketch-constraints-copy")),
                "{:?}",
                reading.sketches[0].refusal
            );

            // A replacement: the height of the plate is its ordinary edit.
            let stored = stored_constraints(&d, sketch);
            let closures: Vec<_> = stored
                .iter()
                .filter(|c| matches!(c.rule, crate::SketchConstraintRule::Coincident { .. }))
                .copied()
                .collect();
            assert_eq!(closures.len(), 4);
            let user: Vec<_> = stored
                .iter()
                .filter(|c| !matches!(c.rule, crate::SketchConstraintRule::Coincident { .. }))
                .map(|c| c.id)
                .collect();
            assert_eq!(user.len(), 1);
            let swap = crate::SketchConstraintEdits {
                remove: user.clone(),
                add: vec![line_rule(lines[1], crate::LineConstraintKind::Vertical)],
            };
            let p = crate::prepare_sketch_constraints(&d, sketch, &swap).expect("replace");
            assert_eq!((p.added.len(), p.removed.clone()), (1, user.clone()));
            assert_eq!(p.fillet().map(|f| f.feature), Some(first));
            assert_eq!(p.fillets().get(1).map(|f| f.feature), Some(second));
            let before = cells(&d);
            d.write_sketch_constraints(&p).expect("replaced");
            only_the_constraints_moved(&before, &cells(&d), sketch, 0);
            let now = stored_constraints(&d, sketch);
            assert!(
                closures.iter().all(|c| now.contains(c)),
                "closure links keep their UUIDs"
            );
            assert!(
                !now.iter().any(|c| user.contains(&c.id)),
                "the replaced rule is gone"
            );

            // Removing every user constraint leaves the closure links; the
            // Sketch's coordinates (§28J) are offered again under both Fillets.
            let added: Vec<_> = now
                .iter()
                .filter(|c| !matches!(c.rule, crate::SketchConstraintRule::Coincident { .. }))
                .map(|c| c.id)
                .collect();
            let strip = crate::SketchConstraintEdits {
                remove: added,
                add: Vec::new(),
            };
            let p = crate::prepare_sketch_constraints(&d, sketch, &strip).expect("strip");
            let before = cells(&d);
            d.write_sketch_constraints(&p).expect("stripped");
            only_the_constraints_moved(&before, &cells(&d), sketch, 0);
            assert_eq!(stored_constraints(&d, sketch), closures);
            assert_eq!(rows(&d), (f1, f2, extrude), "unchanged rows");
            let reading = crate::ExtrudeEditSource::read(&d).expect("catalogue");
            let coords = &reading.sketches[0];
            assert_eq!(coords.refusal, None, "{coords:?}");
            assert_eq!(coords.fillets.get(1).map(|f| f.feature), Some(second));
            assert_eq!(reading.constraint_sketches[0].refusal, None);
            assert!(d.validate().expect("validate").is_ok());
        }
    }

    /// §28K: the constraint writer re-derives both Fillets. A payload that
    /// moves a curve, adds a constraint the preparation did not, or was
    /// prepared before **either** Fillet's radius changed, is refused and
    /// writes nothing — Fillet 2 alone changing is enough, so Fillet 2 is
    /// compared and not merely read.
    #[test]
    fn the_constraint_writer_rederives_both_fillets_and_refuses_forgery() {
        let (_root, mut d, first, second) = sequential(2.375, 2, 3.0625);
        let (sketch, lines) = plate_lines(&d);
        let edits = crate::SketchConstraintEdits {
            remove: Vec::new(),
            add: vec![line_rule(lines[0], crate::LineConstraintKind::Horizontal)],
        };
        let good = crate::prepare_sketch_constraints(&d, sketch, &edits).expect("prepared");
        let untouched = cells(&d);
        let refuse = |d: &mut Document, p: &crate::PreparedSketchConstraints, why: &str| {
            let e = d.write_sketch_constraints(p).expect_err(why);
            assert_eq!(e.kind(), ErrorKind::Input, "{why}: {e}");
            assert_eq!(cells(d), untouched, "{why} wrote something");
            e
        };
        let mut moved = good.clone();
        let ObjectPayload::Sketch(s) = &mut moved.object.payload else {
            unreachable!()
        };
        s.curves = built(
            &lines,
            [[-4.5, 3.25], [40., 3.25], [40., 15.5], [-4.5, 15.5]],
        );
        refuse(&mut d, &moved, "moved curves");
        let mut extra = good.clone();
        let ObjectPayload::Sketch(s) = &mut extra.object.payload else {
            unreachable!()
        };
        s.constraints.push(crate::SketchConstraint {
            id: StableEntityId::new(),
            rule: crate::SketchConstraintRule::Vertical {
                a: crate::SketchPointRef::new(lines[2], crate::SketchPointSelector::Start),
                b: crate::SketchPointRef::new(lines[2], crate::SketchPointSelector::End),
            },
        });
        refuse(&mut d, &extra, "extra");
        // Stale: Fillet 2 alone, then Fillet 1 alone, changed after preparation.
        for (feature, radius) in [(second, 3.5), (first, 2.0)] {
            let edit = prepare_fillet_radius(&d, feature, radius).expect("radius");
            d.write_fillet_radius(&edit).expect("written");
            let changed = cells(&d);
            let e = d.write_sketch_constraints(&good).expect_err("stale");
            assert_eq!(e.kind(), ErrorKind::Input, "{e}");
            assert!(e.to_string().contains("Fillet"), "{e}");
            assert_eq!(cells(&d), changed, "stale wrote something");
        }
    }

    /// §28K: the evaluator already judges both Fillets and their pair on the
    /// Lines the plate was **built** from — the solved ones for a constrained
    /// Sketch — with the stored numbers proving nothing: each radius on its
    /// own corner naming that corner's joint, the flat between adjacent arcs at
    /// the least float that leaves it and one below, opposite corners owing
    /// none, and a Line off its side. No second check is written; this is the
    /// evidence that the existing one is the one asked.
    #[test]
    fn the_evaluator_judges_both_fillets_and_the_pair_on_the_built_lines() {
        // Adjacent, r1 = r2 = 3: each alone fits 6 mm; together they need more.
        let (_a, mut d, first, second) = sequential(3.0, 2, 3.0);
        constrained(&mut d);
        let (_, lines) = plate_lines(&d);
        let objects = d.objects().expect("objects");
        let (one, two) = (stored_fillet(&d, first), stored_fillet(&d, second));
        let judge = |which: &Fillet, at: [[f64; 2]; 4]| {
            crate::evaluable_fillet(&objects, which, Some(&built(&lines, at)))
        };
        let plate = |depth: f64| rect(false, (0., 0.), (20., depth));
        // The part is whatever the solver made: 6 mm deep holds each radius.
        judge(&one, plate(6.0)).expect("Fillet 1 alone fits 2 r");
        let e = judge(&one, plate(5.99)).expect_err("under 2 r1");
        assert_eq!(e.kind(), ErrorKind::Input);
        assert!(e.to_string().contains(&one.edge.joint.to_string()), "{e}");
        let e = judge(&two, plate(5.99)).expect_err("under 2 r2");
        assert!(
            e.to_string().contains("too short") || e.to_string().contains("too large"),
            "{e}"
        );
        // The flat between the two arcs: Fillet 2 is the one that completes
        // the pair, and the shared Line's UUID is what the reason names.
        let e = judge(&two, plate(6.005)).expect_err("no flat left");
        assert_eq!(e.kind(), ErrorKind::Input);
        assert!(e.to_string().contains("flat"), "{e}");
        let shared = crate::fillet::shared_side(
            &crate::corner_for(
                &crate::fillet::corners_of_lines(base_of(&d, first), &built(&lines, plate(9.)))
                    .expect("corners"),
                one.edge,
            )
            .expect("corner"),
            &crate::corner_for(
                &crate::fillet::corners_of_lines(base_of(&d, first), &built(&lines, plate(9.)))
                    .expect("corners"),
                two.edge,
            )
            .expect("corner"),
        )
        .expect("adjacent corners share a Line")
        .0;
        assert!(e.to_string().contains(&shared.to_string()), "{e}");
        let mut least = 6.01_f64;
        while 3.0 > crate::fillet::pair_bound(least, 3.0) {
            least = least.next_up();
        }
        judge(&two, plate(least)).expect("the least depth that leaves the flat");
        assert!(
            judge(&two, plate(least.next_down())).is_err(),
            "one float below"
        );
        // A Line that changes side is refused, naming the side.
        let e = judge(&two, [[20., 0.], [0., 0.], [0., 9.], [20., 9.]]).expect_err("reversed");
        assert!(e.to_string().contains("side"), "{e}");

        // Opposite corners share no Line: only each radius bounds them.
        let (_o, mut d, _, second) = sequential(3.0, 3, 3.0);
        constrained(&mut d);
        let (_, lines) = plate_lines(&d);
        let objects = d.objects().expect("objects");
        let two = stored_fillet(&d, second);
        let judge =
            |at: [[f64; 2]; 4]| crate::evaluable_fillet(&objects, &two, Some(&built(&lines, at)));
        judge(plate(6.0)).expect("no shared Line, no flat owed");
        assert!(judge(plate(5.99)).is_err());

        // Fillet 2's own radius, on its own corner: r1 = 2 fits 10.9 mm and
        // r2 = 5.5 does not, and no pair rule is involved (opposite corners).
        let (_w, mut d, _, second) = sequential(2.0, 3, 5.5);
        constrained(&mut d);
        let (_, lines) = plate_lines(&d);
        let objects = d.objects().expect("objects");
        let two = stored_fillet(&d, second);
        let judge = |depth: f64| {
            crate::evaluable_fillet(&objects, &two, Some(&built(&lines, plate(depth))))
        };
        judge(11.0).expect("exactly 2 r2");
        let e = judge(10.9).expect_err("under 2 r2, over 2 r1");
        assert_eq!(e.kind(), ErrorKind::Input);
        assert!(e.to_string().contains(&two.edge.joint.to_string()), "{e}");
    }

    mod history;
}
