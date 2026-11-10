// SPDX-License-Identifier: MIT
use super::*;

impl Row {
    fn label(&self) -> String {
        format!(
            "{} '{}' ({})",
            self.payload.kind().expect("decoded object kind").label(),
            self.name.as_deref().unwrap_or("Unnamed"),
            self.id
        )
    }
}

impl Candidate {
    pub(crate) fn remove(&mut self, id: ObjectId) -> Result<()> {
        if !self.objects.contains_key(&id) {
            return Err(CadError::input(format!("unknown object {id}")));
        }
        self.removed.insert(id);
        Ok(())
    }

    pub(crate) fn set_suppressed(&mut self, id: ObjectId, suppressed: bool) -> Result<()> {
        let row = self.edit(id)?;
        if !row
            .payload
            .kind()
            .is_some_and(crate::ObjectKind::is_feature)
        {
            return Err(CadError::input(format!(
                "{} cannot be suppressed; only history features can be suppressed",
                row.label()
            )));
        }
        row.suppressed = suppressed;
        Ok(())
    }

    fn surviving_previous(&self, mut next: Option<ObjectId>) -> Option<ObjectId> {
        let mut seen = BTreeSet::new();
        while let Some(id) = next {
            if !self.removed.contains(&id) || !seen.insert(id) {
                break;
            }
            next = self.objects[&id].payload.previous_feature();
        }
        next
    }

    pub(super) fn finish_removals(&mut self, document: &Document) -> Result<()> {
        if self.removed.is_empty() {
            return Ok(());
        }
        let mut conflicts = Vec::new();
        for id in &self.removed {
            if let ObjectPayload::Body(body) = &self.objects[id].payload {
                let mut next = body.tip_feature;
                let mut seen = BTreeSet::new();
                let mut remaining = Vec::new();
                while let Some(feature) = next {
                    if !seen.insert(feature) {
                        break;
                    }
                    let row = &self.objects[&feature];
                    if !self.removed.contains(&feature) {
                        remaining.push(row.label());
                    }
                    next = row.payload.previous_feature();
                }
                if !remaining.is_empty() {
                    conflicts.push(format!(
                        "{} still owns {}; remove every history feature explicitly",
                        self.objects[id].label(),
                        remaining.join(", ")
                    ));
                }
            }
        }
        let retained: Vec<_> = self
            .objects
            .keys()
            .filter(|id| !self.removed.contains(id))
            .copied()
            .collect();
        for id in retained {
            let row = &self.objects[&id];
            let previous = match &row.payload {
                ObjectPayload::Body(body) => body.tip_feature,
                payload => payload.previous_feature(),
            };
            let next = self.surviving_previous(previous);
            if next == previous {
                continue;
            }
            let starts_body = matches!(&row.payload,
                ObjectPayload::Extrude(e) if e.operation == crate::SolidOperation::NewBody)
                || matches!(&row.payload,
                    ObjectPayload::Revolve(r) if r.operation == crate::SolidOperation::NewBody);
            if next.is_none() && !matches!(row.payload, ObjectPayload::Body(_)) && !starts_body {
                conflicts.push(format!(
                    "{} cannot remain without a predecessor; its operation does not start a body",
                    row.label()
                ));
                continue;
            }
            match &mut self.edit(id)?.payload {
                ObjectPayload::Body(body) => body.tip_feature = next,
                ObjectPayload::Extrude(feature) => feature.previous = next,
                ObjectPayload::Revolve(feature) => feature.previous = next,
                ObjectPayload::Fillet(feature) => feature.previous = next.expect("predecessor"),
                ObjectPayload::Chamfer(feature) => feature.previous = next.expect("predecessor"),
                ObjectPayload::KeepSolids(feature) => feature.previous = next.expect("predecessor"),
                _ => unreachable!("only history links are rewired"),
            }
        }
        // Resolve text against the full named parameter catalogue before removing its entries.
        let parameters = references(self.objects.iter().map(|(id, o)| (*id, &o.payload)))?;
        let mut edges: BTreeSet<_> = document
            .dependencies()?
            .into_iter()
            .filter(|e| e.role != DependencyRole::Parameter && !self.old.contains_key(&e.dependent))
            .collect();
        edges.extend(parameters);
        for id in self.old.keys() {
            edges.extend(self.objects[id].payload.dependencies(*id));
        }
        let mut users = BTreeMap::<ObjectId, BTreeSet<ObjectId>>::new();
        for edge in edges {
            if self.removed.contains(&edge.dependency) && !self.removed.contains(&edge.dependent) {
                users
                    .entry(edge.dependency)
                    .or_default()
                    .insert(edge.dependent);
            }
        }
        for row in self.objects.values() {
            if let Some(parent) = row.parent
                && self.removed.contains(&parent)
                && !self.removed.contains(&row.id)
            {
                users.entry(parent).or_default().insert(row.id);
            }
        }
        for (id, dependents) in users {
            let labels: Vec<_> = dependents
                .iter()
                .map(|id| self.objects[id].label())
                .collect();
            conflicts.push(format!(
                "{} is referenced by {}",
                self.objects[&id].label(),
                labels.join(", ")
            ));
        }
        if !conflicts.is_empty() {
            return Err(CadError::input(conflicts.join("; ")));
        }
        for id in &self.removed {
            self.objects.remove(id);
            self.old.remove(id);
            self.added.remove(id);
        }
        Ok(())
    }
}
