use std::collections::{BTreeMap, BTreeSet};

use ferritecad_types::{CadError, ObjectId, Result};

/// Why one object depends on another.
///
/// The role is part of the edge's identity, so a feature can depend on the same
/// object twice for different reasons without the two collapsing into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum DependencyRole {
    /// The sketch supplying a feature's profile.
    Profile,
    /// The datum a sketch or feature is placed on.
    Plane,
    /// The body a feature modifies.
    TargetBody,
    /// The feature whose result a body exposes as its current tip.
    BodyTip,
    /// A named value an expression reads.
    Parameter,
    /// Geometry named through a topology reference.
    TopologyReference,
}

impl DependencyRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Plane => "plane",
            Self::TargetBody => "target_body",
            Self::BodyTip => "body_tip",
            Self::Parameter => "parameter",
            Self::TopologyReference => "topology_reference",
        }
    }

    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "profile" => Ok(Self::Profile),
            "plane" => Ok(Self::Plane),
            "target_body" => Ok(Self::TargetBody),
            "body_tip" => Ok(Self::BodyTip),
            "parameter" => Ok(Self::Parameter),
            "topology_reference" => Ok(Self::TopologyReference),
            other => Err(CadError::input(format!(
                "unknown dependency role {other:?}"
            ))),
        }
    }
}

/// A directed edge: `dependent` cannot be evaluated before `dependency`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Dependency {
    pub dependent: ObjectId,
    pub dependency: ObjectId,
    pub role: DependencyRole,
}

/// Orders objects so every dependency precedes its dependents.
///
/// The result is deterministic: among objects that are ready at the same time,
/// the smaller identifier goes first. Since identifiers are UUIDv7 this means
/// creation order, and it means two machines rebuilding the same document
/// schedule it identically — a prerequisite for comparing rebuild results at
/// all.
pub fn evaluation_order(nodes: &[ObjectId], deps: &[Dependency]) -> Result<Vec<ObjectId>> {
    let known: BTreeSet<ObjectId> = nodes.iter().copied().collect();

    let mut dependents: BTreeMap<ObjectId, BTreeSet<ObjectId>> = BTreeMap::new();
    let mut remaining: BTreeMap<ObjectId, BTreeSet<ObjectId>> =
        known.iter().map(|id| (*id, BTreeSet::new())).collect();

    for dep in deps {
        if !known.contains(&dep.dependency) {
            return Err(CadError::input(format!(
                "object {} depends on {}, which is not in the graph",
                dep.dependent, dep.dependency
            )));
        }
        let Some(waiting) = remaining.get_mut(&dep.dependent) else {
            return Err(CadError::input(format!(
                "dependency edge names {}, which is not in the graph",
                dep.dependent
            )));
        };
        // Roles are ignored here: two edges between the same pair are one
        // ordering constraint.
        if waiting.insert(dep.dependency) {
            dependents
                .entry(dep.dependency)
                .or_default()
                .insert(dep.dependent);
        }
    }

    let mut ready: BTreeSet<ObjectId> = remaining
        .iter()
        .filter(|(_, waiting)| waiting.is_empty())
        .map(|(id, _)| *id)
        .collect();

    let mut order = Vec::with_capacity(known.len());
    while let Some(next) = ready.iter().next().copied() {
        ready.remove(&next);
        remaining.remove(&next);
        order.push(next);

        for dependent in dependents.get(&next).into_iter().flatten() {
            if let Some(waiting) = remaining.get_mut(dependent) {
                waiting.remove(&next);
                if waiting.is_empty() {
                    ready.insert(*dependent);
                }
            }
        }
    }

    if !remaining.is_empty() {
        let stuck: Vec<String> = remaining.keys().map(ObjectId::to_string).collect();
        return Err(CadError::input(format!(
            "the feature graph contains a cycle among: {}",
            stuck.join(", ")
        )));
    }

    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(dependent: ObjectId, dependency: ObjectId) -> Dependency {
        Dependency {
            dependent,
            dependency,
            role: DependencyRole::Profile,
        }
    }

    #[test]
    fn dependencies_come_before_dependents() {
        let plane = ObjectId::new();
        let sketch = ObjectId::new();
        let extrude = ObjectId::new();

        let order = evaluation_order(
            &[extrude, sketch, plane],
            &[edge(sketch, plane), edge(extrude, sketch)],
        )
        .expect("acyclic graph orders");

        assert_eq!(order, vec![plane, sketch, extrude]);
    }

    #[test]
    fn ordering_is_stable_regardless_of_input_order() {
        let a = ObjectId::new();
        let b = ObjectId::new();
        let c = ObjectId::new();
        let deps = [edge(c, a), edge(c, b)];

        let one = evaluation_order(&[a, b, c], &deps).expect("orders");
        let other = evaluation_order(&[c, b, a], &deps).expect("orders");
        assert_eq!(one, other);
    }

    #[test]
    fn a_cycle_is_reported_with_its_members() {
        let a = ObjectId::new();
        let b = ObjectId::new();

        let err = evaluation_order(&[a, b], &[edge(a, b), edge(b, a)])
            .expect_err("a cycle cannot be ordered");

        let message = err.to_string();
        assert!(message.contains("cycle"));
        assert!(message.contains(&a.to_string()));
        assert!(message.contains(&b.to_string()));
    }

    #[test]
    fn a_missing_dependency_is_reported_rather_than_skipped() {
        let present = ObjectId::new();
        let absent = ObjectId::new();

        let err = evaluation_order(&[present], &[edge(present, absent)])
            .expect_err("a dangling edge is not orderable");
        assert!(err.to_string().contains(&absent.to_string()));
    }

    #[test]
    fn duplicate_edges_do_not_deadlock_the_order() {
        let a = ObjectId::new();
        let b = ObjectId::new();

        let order = evaluation_order(
            &[a, b],
            &[
                Dependency {
                    dependent: b,
                    dependency: a,
                    role: DependencyRole::Profile,
                },
                Dependency {
                    dependent: b,
                    dependency: a,
                    role: DependencyRole::Plane,
                },
            ],
        )
        .expect("parallel edges are one constraint");

        assert_eq!(order, vec![a, b]);
    }

    #[test]
    fn an_empty_graph_orders_to_nothing() {
        assert!(
            evaluation_order(&[], &[])
                .expect("trivially ordered")
                .is_empty()
        );
    }
}
