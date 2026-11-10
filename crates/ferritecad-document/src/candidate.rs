// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_types::{CadError, ObjectId, Result};

use crate::formula::check_name;
use crate::parameters::{ParameterEdges, names_in, references, rename_references};
use crate::{
    Dependency, DependencyRole, Document, DocumentWriter, ObjectPayload, Parameter, ParameterValues,
};

mod tree;

pub(crate) struct Row {
    id: ObjectId,
    parent: Option<ObjectId>,
    ordinal: i64,
    pub(crate) name: Option<String>,
    pub(crate) payload: ObjectPayload,
    suppressed: bool,
    saved: Option<Vec<u8>>,
}

pub(crate) struct Candidate {
    objects: BTreeMap<ObjectId, Row>,
    // Keep exactly the old outgoing payload links of edited rows. Unrelated rows retain their IDs.
    old: BTreeMap<ObjectId, Vec<Dependency>>,
    added: BTreeSet<ObjectId>,
    removed: BTreeSet<ObjectId>,
    ordinal: i64,
}

impl Candidate {
    pub(crate) fn read(document: &Document) -> Result<Self> {
        let objects = document.objects()?;
        let ordinal = objects.iter().map(|o| o.ordinal).max().unwrap_or(0);
        Ok(Self {
            objects: objects
                .into_iter()
                .map(|o| {
                    (
                        o.id,
                        Row {
                            id: o.id,
                            parent: o.parent,
                            ordinal: o.ordinal,
                            name: o.name,
                            payload: o.payload,
                            suppressed: o.suppressed,
                            saved: Some(o.storage_bytes),
                        },
                    )
                })
                .collect(),
            old: BTreeMap::new(),
            added: BTreeSet::new(),
            removed: BTreeSet::new(),
            ordinal,
        })
    }

    pub(crate) fn edit(&mut self, id: ObjectId) -> Result<&mut Row> {
        let obj = self
            .objects
            .get_mut(&id)
            .ok_or_else(|| CadError::input(format!("unknown object {id}")))?;
        if let std::collections::btree_map::Entry::Vacant(entry) = self.old.entry(id) {
            if obj.saved.as_deref()
                != Some(obj.payload.encode_suppressed(obj.suppressed)?.as_slice())
            {
                return Err(CadError::unsupported(format!(
                    "object {id}'s saved payload cannot be rewritten losslessly by this build"
                )));
            }
            entry.insert(obj.payload.dependencies(id));
        }
        Ok(obj)
    }

    pub(crate) fn check_name(&self, id: ObjectId, name: &str) -> Result<()> {
        check_name(name)?;
        if self.objects.values().any(|o| {
            o.id != id && matches!(&o.payload, ObjectPayload::Parameter(p) if p.name == name)
        }) {
            return Err(CadError::input(format!(
                "duplicate parameter name '{name}'"
            )));
        }
        Ok(())
    }

    pub(crate) fn parameter_mut(&mut self, id: ObjectId) -> Result<&mut Parameter> {
        match &mut self.edit(id)?.payload {
            ObjectPayload::Parameter(p) => Ok(p),
            _ => Err(CadError::input(format!("object {id} is not a parameter"))),
        }
    }

    pub(crate) fn add(
        &mut self,
        id: ObjectId,
        name: &Option<String>,
        payload: ObjectPayload,
    ) -> Result<()> {
        if self.objects.contains_key(&id) || self.removed.contains(&id) {
            return Err(CadError::input(format!("object {id} already exists")));
        }
        self.ordinal = self
            .ordinal
            .checked_add(1)
            .ok_or_else(|| CadError::input("new object ordinal overflows"))?;
        self.objects.insert(
            id,
            Row {
                id,
                parent: None,
                ordinal: self.ordinal,
                name: name.clone(),
                payload,
                suppressed: false,
                saved: None,
            },
        );
        self.old.insert(id, Vec::new());
        self.added.insert(id);
        Ok(())
    }

    fn users(&self, name: &str) -> Result<Vec<ObjectId>> {
        let mut ids = Vec::new();
        for o in self.objects.values() {
            if names_in(o.id, &o.payload, name)? {
                ids.push(o.id);
            }
        }
        Ok(ids)
    }

    pub(crate) fn rename(&mut self, id: ObjectId, name: &str) -> Result<()> {
        self.check_name(id, name)?;
        let p = self.parameter_mut(id)?;
        let old = std::mem::replace(&mut p.name, name.to_owned());
        for owner in self.users(&old)? {
            rename_references(owner, &mut self.edit(owner)?.payload, &old, name)?;
        }
        let o = self.edit(id)?;
        if o.name.as_deref() == Some(&old) {
            o.name = Some(name.to_owned());
        }
        Ok(())
    }

    fn validate_target(&self, edge: Dependency) -> Result<()> {
        let target = self
            .objects
            .get(&edge.dependency)
            .ok_or_else(|| CadError::input(format!("unknown object {}", edge.dependency)))?;
        let valid = match edge.role {
            DependencyRole::Plane => matches!(target.payload, ObjectPayload::DatumPlane(_)),
            DependencyRole::Profile => matches!(target.payload, ObjectPayload::Sketch(_)),
            DependencyRole::Predecessor | DependencyRole::BodyTip | DependencyRole::Support => {
                target
                    .payload
                    .kind()
                    .is_some_and(crate::ObjectKind::is_feature)
            }
            DependencyRole::Parameter => matches!(target.payload, ObjectPayload::Parameter(_)),
        };
        if !valid {
            return Err(CadError::input(format!(
                "object {} has the wrong kind for {}",
                edge.dependency,
                edge.role.as_str()
            )));
        }
        Ok(())
    }

    /// Evaluate parameter dependencies before their users. An unrelated failed object stays
    /// repairable. Its stale value is never used to compute a newly edited dependent.
    fn evaluate(&mut self, edges: &ParameterEdges) -> Result<()> {
        let nodes: Vec<_> = self.objects.keys().copied().collect();
        let links: Vec<_> = edges.iter().copied().collect();
        let order = crate::graph::order_with_cycle(&nodes, &links, |remaining| {
            let mut current = *remaining.keys().next().expect("unresolved graph");
            let mut path = Vec::new();
            let mut seen = BTreeMap::new();
            loop {
                if let Some(start) = seen.insert(current, path.len()) {
                    path.push(current);
                    let names: Vec<_> = path[start..]
                        .iter()
                        .map(|id| {
                            let ObjectPayload::Parameter(p) = &self.objects[id].payload else {
                                unreachable!("only parameters can close a parameter cycle")
                            };
                            p.name.as_str()
                        })
                        .collect();
                    break format!("parameter cycle: {}", names.join(" → "));
                }
                path.push(current);
                current = *remaining[&current].first().expect("unresolved dependency");
            }
        })?;
        let mut values = ParameterValues::default();
        let mut failed = BTreeSet::new();
        let mut reasons = Vec::new();
        let mut affected: BTreeSet<_> = self.old.keys().copied().collect();
        let mut inputs = BTreeMap::<ObjectId, Vec<ObjectId>>::new();
        for e in edges {
            inputs.entry(e.dependent).or_default().push(e.dependency);
        }
        for id in order {
            let deps = inputs.get(&id).map(Vec::as_slice).unwrap_or_default();
            let dirty = affected.contains(&id) || deps.iter().any(|d| affected.contains(d));
            if dirty {
                affected.insert(id);
            }
            let p = &self.objects[&id].payload;
            if !dirty && !matches!(p, ObjectPayload::Parameter(_)) {
                continue;
            }
            let result = if let Some(bad) = deps.iter().find(|d| failed.contains(*d)) {
                let ObjectPayload::Parameter(dep) = &self.objects[bad].payload else {
                    unreachable!("resolved parameter")
                };
                let owner = match p {
                    ObjectPayload::Parameter(p) => format!("parameter {}", p.name),
                    _ => p.type_name().to_owned(),
                };
                Err(CadError::input(format!(
                    "object {id} {owner} depends on parameter {}, which failed",
                    dep.name
                )))
            } else if dirty {
                values.update(id, &mut self.edit(id)?.payload)
            } else {
                let ObjectPayload::Parameter(parameter) = p else {
                    unreachable!("unmodified parameter")
                };
                values.verify(id, p).and_then(|()| values.bind(parameter))
            };
            if let Err(e) = result {
                failed.insert(id);
                if dirty {
                    reasons.push(e.to_string());
                }
            }
        }
        if !reasons.is_empty() {
            return Err(CadError::input(reasons.join("; ")));
        }
        Ok(())
    }

    pub(crate) fn write(
        mut self,
        document: &Document,
        writer: &mut DocumentWriter<'_>,
    ) -> Result<()> {
        self.finish_removals(document)?;
        let new_parameters = references(self.objects.iter().map(|(id, o)| (*id, &o.payload)))?;
        self.evaluate(&new_parameters)?;
        let mut old: BTreeSet<_> = document
            .dependencies()?
            .into_iter()
            .filter(|e| e.role == DependencyRole::Parameter)
            .collect();
        let mut new = new_parameters;
        let mut ordered: Vec<_> = self.old.keys().map(|id| &self.objects[id]).collect();
        ordered.sort_by_key(|o| o.ordinal);
        for o in ordered {
            o.payload.validate()?;
            let original = &self.old[&o.id];
            for edge in o.payload.dependencies(o.id) {
                if !original.contains(&edge) {
                    self.validate_target(edge)?;
                }
                new.insert(edge);
            }
            old.extend(original);
            if self.added.contains(&o.id) || matches!(o.payload, ObjectPayload::Parameter(_)) {
                writer.put_object_state(
                    o.id,
                    o.parent,
                    o.ordinal,
                    o.name.as_deref(),
                    (&o.payload, o.suppressed),
                )?;
            } else {
                writer.update_payload(o.id, &o.payload, o.suppressed)?;
            }
        }
        for edge in old.difference(&new) {
            writer.remove_dependency(*edge)?;
        }
        for edge in new.difference(&old) {
            writer.add_dependency(*edge)?;
        }
        for edge in document.dependencies()? {
            if self.removed.contains(&edge.dependent) {
                writer.remove_dependency(edge)?;
            }
        }
        for id in self.removed {
            writer.remove_object(id)?;
        }
        Ok(())
    }
}
