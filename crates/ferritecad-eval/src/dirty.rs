use std::collections::{BTreeMap, BTreeSet};

use ferritecad_document::Dependency;
use ferritecad_types::{CadError, ObjectId, Result};

/// The dependency graph read backwards: for each object, who reads it.
///
/// The stored edges point from dependent to dependency, which is the direction
/// evaluation needs. Propagating staleness needs the opposite direction, so it
/// is built once and reused rather than rediscovered per query.
///
/// Roles are dropped on the way in. Two objects connected by three edges for
/// three different reasons are still one propagation path.
#[derive(Debug, Clone, Default)]
pub struct DependentIndex {
    nodes: BTreeSet<ObjectId>,
    dependents: BTreeMap<ObjectId, BTreeSet<ObjectId>>,
}

impl DependentIndex {
    /// Builds the reverse index, refusing an edge that names an object outside
    /// `nodes`.
    ///
    /// A dangling edge is reported rather than dropped. Silently ignoring one
    /// would mean silently not rebuilding whatever hung off it, and an object
    /// left stale while the interface says the rebuild succeeded is precisely
    /// the failure this whole layer exists to prevent.
    pub fn build(nodes: &[ObjectId], deps: &[Dependency]) -> Result<Self> {
        let nodes: BTreeSet<ObjectId> = nodes.iter().copied().collect();
        let mut dependents: BTreeMap<ObjectId, BTreeSet<ObjectId>> = BTreeMap::new();

        for dep in deps {
            if !nodes.contains(&dep.dependency) {
                return Err(CadError::input(format!(
                    "object {} depends on {}, which is not in the graph",
                    dep.dependent, dep.dependency
                )));
            }
            if !nodes.contains(&dep.dependent) {
                return Err(CadError::input(format!(
                    "dependency edge names {}, which is not in the graph",
                    dep.dependent
                )));
            }
            dependents
                .entry(dep.dependency)
                .or_default()
                .insert(dep.dependent);
        }

        Ok(Self { nodes, dependents })
    }

    /// Every object in the graph, in identifier order.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = ObjectId> + '_ {
        self.nodes.iter().copied()
    }

    pub fn contains(&self, id: ObjectId) -> bool {
        self.nodes.contains(&id)
    }

    /// The objects that read `id` directly, in identifier order.
    pub fn direct_dependents(&self, id: ObjectId) -> impl ExactSizeIterator<Item = ObjectId> + '_ {
        self.dependents
            .get(&id)
            .map(|set| set.iter())
            .unwrap_or_default()
            .copied()
    }

    /// Everything made stale by `changed`, including `changed` itself.
    ///
    /// Refuses an identifier the graph does not contain: a caller reporting a
    /// change to an object we have never seen is out of step with the document,
    /// and quietly dropping it would under-rebuild.
    pub fn dirty_set(&self, changed: &[ObjectId]) -> Result<BTreeSet<ObjectId>> {
        let mut dirty = BTreeSet::new();
        let mut frontier = Vec::new();

        for id in changed {
            if !self.nodes.contains(id) {
                return Err(CadError::input(format!(
                    "object {id} was reported as changed but is not in the graph"
                )));
            }
            if dirty.insert(*id) {
                frontier.push(*id);
            }
        }

        // Breadth of the walk does not affect the result: `dirty` is a set and
        // an object is enqueued only on the transition from clean to dirty, so
        // a diamond is visited once and a cycle terminates.
        while let Some(current) = frontier.pop() {
            for dependent in self.direct_dependents(current) {
                if dirty.insert(dependent) {
                    frontier.push(dependent);
                }
            }
        }

        Ok(dirty)
    }
}

/// Everything made stale by `changed`, over a graph given as slices.
///
/// Convenience over [`DependentIndex::build`] for callers with one query to
/// make; build the index directly when asking repeatedly.
pub fn dirty_set(
    nodes: &[ObjectId],
    deps: &[Dependency],
    changed: &[ObjectId],
) -> Result<BTreeSet<ObjectId>> {
    DependentIndex::build(nodes, deps)?.dirty_set(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferritecad_document::DependencyRole;

    fn edge(dependent: ObjectId, dependency: ObjectId) -> Dependency {
        Dependency {
            dependent,
            dependency,
            role: DependencyRole::Profile,
        }
    }

    /// Four identifiers in creation order, so tests can assert on ordering.
    fn ids<const N: usize>() -> [ObjectId; N] {
        std::array::from_fn(|_| ObjectId::new())
    }

    #[test]
    fn a_change_makes_everything_downstream_stale() {
        let [plane, sketch, extrude, body] = ids();
        let deps = [
            edge(sketch, plane),
            edge(extrude, sketch),
            edge(body, extrude),
        ];

        let dirty = dirty_set(&[plane, sketch, extrude, body], &deps, &[sketch])
            .expect("the graph is well formed");

        assert_eq!(
            dirty,
            BTreeSet::from([sketch, extrude, body]),
            "the plane is upstream of the change and stays clean"
        );
    }

    #[test]
    fn an_independent_branch_stays_clean() {
        let [
            plane,
            left_sketch,
            right_sketch,
            left_extrude,
            right_extrude,
        ] = ids();
        let nodes = [
            plane,
            left_sketch,
            right_sketch,
            left_extrude,
            right_extrude,
        ];
        let deps = [
            edge(left_sketch, plane),
            edge(right_sketch, plane),
            edge(left_extrude, left_sketch),
            edge(right_extrude, right_sketch),
        ];

        let dirty = dirty_set(&nodes, &deps, &[left_sketch]).expect("well formed");

        assert_eq!(dirty, BTreeSet::from([left_sketch, left_extrude]));
        assert!(!dirty.contains(&right_sketch));
        assert!(!dirty.contains(&right_extrude));
    }

    #[test]
    fn several_change_roots_union_their_reach() {
        let [a, b, a_child, b_child, shared] = ids();
        let nodes = [a, b, a_child, b_child, shared];
        let deps = [
            edge(a_child, a),
            edge(b_child, b),
            edge(shared, a_child),
            edge(shared, b_child),
        ];

        let dirty = dirty_set(&nodes, &deps, &[a, b]).expect("well formed");
        assert_eq!(dirty, BTreeSet::from([a, b, a_child, b_child, shared]));
    }

    #[test]
    fn a_diamond_is_walked_once_and_completely() {
        let [root, left, right, join] = ids();
        let deps = [
            edge(left, root),
            edge(right, root),
            edge(join, left),
            edge(join, right),
        ];

        let dirty = dirty_set(&[root, left, right, join], &deps, &[root]).expect("well formed");
        assert_eq!(dirty, BTreeSet::from([root, left, right, join]));
    }

    #[test]
    fn changing_nothing_makes_nothing_stale() {
        let [a, b] = ids();
        let dirty = dirty_set(&[a, b], &[edge(b, a)], &[]).expect("well formed");
        assert!(dirty.is_empty());
    }

    #[test]
    fn a_leaf_change_touches_only_itself() {
        let [plane, sketch, extrude] = ids();
        let deps = [edge(sketch, plane), edge(extrude, sketch)];

        let dirty = dirty_set(&[plane, sketch, extrude], &deps, &[extrude]).expect("well formed");
        assert_eq!(dirty, BTreeSet::from([extrude]));
    }

    #[test]
    fn the_result_does_not_depend_on_input_order() {
        let [a, b, c, d] = ids();
        let deps = [edge(b, a), edge(c, b), edge(d, a)];
        let reversed: Vec<Dependency> = deps.iter().rev().copied().collect();

        let one = dirty_set(&[a, b, c, d], &deps, &[a]).expect("well formed");
        let other = dirty_set(&[d, c, b, a], &reversed, &[a]).expect("well formed");
        assert_eq!(one, other);
    }

    #[test]
    fn a_dangling_dependency_is_refused() {
        let [present, absent] = ids();
        let err = DependentIndex::build(&[present], &[edge(present, absent)])
            .expect_err("an edge into nothing is not a graph");

        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Input);
        assert!(err.to_string().contains(&absent.to_string()));
    }

    #[test]
    fn a_dangling_dependent_is_refused() {
        let [present, absent] = ids();
        let err = DependentIndex::build(&[present], &[edge(absent, present)])
            .expect_err("an edge from nothing is not a graph");

        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Input);
        assert!(err.to_string().contains(&absent.to_string()));
    }

    #[test]
    fn a_change_to_an_unknown_object_is_refused() {
        let [known, unknown] = ids();
        let err = dirty_set(&[known], &[], &[unknown])
            .expect_err("a change we cannot place must not be dropped");

        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Input);
        assert!(err.to_string().contains(&unknown.to_string()));
    }

    #[test]
    fn a_cycle_terminates_instead_of_looping() {
        // Propagation must not hang even on a graph that planning will reject;
        // the honest error belongs to the planner, not to an infinite loop.
        let [a, b] = ids();
        let dirty = dirty_set(&[a, b], &[edge(a, b), edge(b, a)], &[a]).expect("propagation ends");
        assert_eq!(dirty, BTreeSet::from([a, b]));
    }

    #[test]
    fn parallel_edges_are_one_path() {
        let [a, b] = ids();
        let deps = [
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
        ];

        let index = DependentIndex::build(&[a, b], &deps).expect("well formed");
        assert_eq!(index.direct_dependents(a).collect::<Vec<_>>(), vec![b]);
    }

    #[test]
    fn an_object_nothing_reads_has_no_dependents() {
        let [a, b] = ids();
        let index = DependentIndex::build(&[a, b], &[edge(b, a)]).expect("well formed");

        assert_eq!(index.direct_dependents(b).count(), 0);
        assert!(index.contains(a));
        assert_eq!(index.nodes().collect::<Vec<_>>(), vec![a, b]);
    }
}
