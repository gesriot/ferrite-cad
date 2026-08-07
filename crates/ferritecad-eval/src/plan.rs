// SPDX-License-Identifier: MIT
use std::collections::{BTreeMap, BTreeSet};

use ferritecad_document::{Dependency, evaluation_order};
use ferritecad_types::{ObjectId, Result};

use crate::dirty::DependentIndex;

/// What to rebuild, in an order that is safe and in groups that are parallel.
///
/// A plan contains only stale objects. Clean objects are absent by design: an
/// object in the plan may depend on one that is not, and that is the normal
/// case — the clean result is already computed and is what the cache holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RebuildPlan {
    order: Vec<ObjectId>,
    levels: Vec<Vec<ObjectId>>,
}

impl RebuildPlan {
    /// The stale objects in a valid sequential rebuild order.
    ///
    /// This is the document's own evaluation order with the clean objects
    /// removed, so it agrees with a full rebuild rather than being a second
    /// opinion about ordering.
    pub fn order(&self) -> &[ObjectId] {
        &self.order
    }

    /// The same objects grouped into waves that may run concurrently.
    ///
    /// No object in a level depends on another in the same level, so a level
    /// can be handed to as many workers as are available. Levels run in
    /// sequence, and within a level the objects are in identifier order, which
    /// for UUIDv7 means creation order.
    ///
    /// Nothing here starts a thread. This is the shape a scheduler will need;
    /// the scheduler itself belongs to the stage that has real work to run.
    pub fn levels(&self) -> &[Vec<ObjectId>] {
        &self.levels
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// How many objects need rebuilding.
    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn contains(&self, id: ObjectId) -> bool {
        self.order.contains(&id)
    }
}

/// Plans the rebuild caused by changing `changed`.
///
/// Fails, rather than returning a partial plan, when the graph has a cycle or
/// an edge into an object that is not there. A plan that quietly omits work is
/// worse than no plan: it produces a model the interface believes is up to date
/// and which is not.
pub fn plan_rebuild(
    nodes: &[ObjectId],
    deps: &[Dependency],
    changed: &[ObjectId],
) -> Result<RebuildPlan> {
    let index = DependentIndex::build(nodes, deps)?;
    let dirty = index.dirty_set(changed)?;

    // Ordering the whole graph rather than the dirty subgraph is deliberate.
    // It reuses the one topological sort the document already tests, and it is
    // what makes a partial plan a subsequence of the full one instead of a
    // separately derived answer that might disagree.
    let full_order = evaluation_order(nodes, deps)?;

    let order: Vec<ObjectId> = full_order
        .into_iter()
        .filter(|id| dirty.contains(id))
        .collect();
    let levels = levels_of(&order, &dirty, deps);

    Ok(RebuildPlan { order, levels })
}

/// Plans a rebuild of everything, as a cold rebuild with no cache does.
pub fn plan_full_rebuild(nodes: &[ObjectId], deps: &[Dependency]) -> Result<RebuildPlan> {
    plan_rebuild(nodes, deps, nodes)
}

/// Groups an already ordered dirty set into non-interfering waves.
///
/// An object's level is one past the deepest level among its *stale*
/// dependencies. Clean dependencies contribute nothing: their results already
/// exist, so they impose no waiting.
fn levels_of(
    order: &[ObjectId],
    dirty: &BTreeSet<ObjectId>,
    deps: &[Dependency],
) -> Vec<Vec<ObjectId>> {
    let mut stale_inputs: BTreeMap<ObjectId, BTreeSet<ObjectId>> = BTreeMap::new();
    for dep in deps {
        if dirty.contains(&dep.dependent) && dirty.contains(&dep.dependency) {
            stale_inputs
                .entry(dep.dependent)
                .or_default()
                .insert(dep.dependency);
        }
    }

    let mut level_of: BTreeMap<ObjectId, usize> = BTreeMap::new();
    let mut levels: Vec<Vec<ObjectId>> = Vec::new();

    // `order` is topological, so every stale input of an object already has a
    // level by the time the object is reached and no lookup can miss.
    for id in order {
        let level = stale_inputs
            .get(id)
            .into_iter()
            .flatten()
            .filter_map(|input| level_of.get(input))
            .map(|depth| depth + 1)
            .max()
            .unwrap_or(0);

        level_of.insert(*id, level);
        if levels.len() <= level {
            levels.resize(level + 1, Vec::new());
        }
        levels[level].push(*id);
    }

    // `order` visits identifiers in ascending order within each ready wave, so
    // each level is already sorted; sorting again keeps that a property of this
    // function rather than an assumption about the caller.
    for level in &mut levels {
        level.sort_unstable();
    }

    levels
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

    fn ids<const N: usize>() -> [ObjectId; N] {
        std::array::from_fn(|_| ObjectId::new())
    }

    /// Asserts that every dependency inside the plan precedes its dependent.
    fn assert_well_ordered(plan: &RebuildPlan, deps: &[Dependency]) {
        let position: BTreeMap<ObjectId, usize> = plan
            .order()
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i))
            .collect();

        for dep in deps {
            if let (Some(dependent), Some(dependency)) =
                (position.get(&dep.dependent), position.get(&dep.dependency))
            {
                assert!(
                    dependency < dependent,
                    "a dependency must be rebuilt before what reads it"
                );
            }
        }
    }

    #[test]
    fn a_linear_chain_rebuilds_from_the_change_downwards() {
        let [plane, sketch, extrude, body] = ids();
        let nodes = [plane, sketch, extrude, body];
        let deps = [
            edge(sketch, plane),
            edge(extrude, sketch),
            edge(body, extrude),
        ];

        let plan = plan_rebuild(&nodes, &deps, &[sketch]).expect("well formed");

        assert_eq!(plan.order(), &[sketch, extrude, body]);
        assert!(!plan.contains(plane), "the plane did not change");
        assert_eq!(plan.len(), 3);
        assert_well_ordered(&plan, &deps);
    }

    #[test]
    fn a_stale_object_may_depend_on_a_clean_one() {
        let [plane, sketch, extrude] = ids();
        let nodes = [plane, sketch, extrude];
        let deps = [edge(sketch, plane), edge(extrude, sketch)];

        // Only the extrude changed. Its profile is clean and cached, and must
        // not be dragged into the plan to satisfy the ordering.
        let plan = plan_rebuild(&nodes, &deps, &[extrude]).expect("well formed");

        assert_eq!(plan.order(), &[extrude]);
        assert_eq!(plan.levels(), &[vec![extrude]]);
    }

    #[test]
    fn changing_one_branch_leaves_the_other_out_of_the_plan() {
        let [plane, left, right, left_tip, right_tip] = ids();
        let nodes = [plane, left, right, left_tip, right_tip];
        let deps = [
            edge(left, plane),
            edge(right, plane),
            edge(left_tip, left),
            edge(right_tip, right),
        ];

        let plan = plan_rebuild(&nodes, &deps, &[left]).expect("well formed");

        assert_eq!(plan.order(), &[left, left_tip]);
        assert!(!plan.contains(right));
        assert!(!plan.contains(right_tip));
    }

    #[test]
    fn several_change_roots_produce_one_merged_plan() {
        let [a, b, a_tip, b_tip, join] = ids();
        let nodes = [a, b, a_tip, b_tip, join];
        let deps = [
            edge(a_tip, a),
            edge(b_tip, b),
            edge(join, a_tip),
            edge(join, b_tip),
        ];

        let plan = plan_rebuild(&nodes, &deps, &[a, b]).expect("well formed");

        assert_eq!(plan.len(), 5);
        assert_eq!(plan.order().last().copied(), Some(join));
        assert_well_ordered(&plan, &deps);
    }

    #[test]
    fn independent_objects_share_a_level() {
        let [root, left, right, join] = ids();
        let nodes = [root, left, right, join];
        let deps = [
            edge(left, root),
            edge(right, root),
            edge(join, left),
            edge(join, right),
        ];

        let plan = plan_full_rebuild(&nodes, &deps).expect("well formed");

        // Sorted, because `left` and `right` were created in that order.
        let mut middle = vec![left, right];
        middle.sort_unstable();

        assert_eq!(plan.levels(), &[vec![root], middle, vec![join]]);
    }

    #[test]
    fn no_two_objects_in_a_level_depend_on_each_other() {
        let [a, b, c, d, e] = ids();
        let nodes = [a, b, c, d, e];
        let deps = [edge(b, a), edge(c, a), edge(d, b), edge(e, d)];

        let plan = plan_full_rebuild(&nodes, &deps).expect("well formed");

        let level_of: BTreeMap<ObjectId, usize> = plan
            .levels()
            .iter()
            .enumerate()
            .flat_map(|(depth, level)| level.iter().map(move |id| (*id, depth)))
            .collect();

        for dep in &deps {
            assert!(
                level_of[&dep.dependency] < level_of[&dep.dependent],
                "an edge inside one level would mean two workers racing"
            );
        }
    }

    #[test]
    fn every_object_appears_in_exactly_one_level() {
        let [a, b, c, d] = ids();
        let nodes = [a, b, c, d];
        let deps = [edge(b, a), edge(c, b), edge(d, a)];

        let plan = plan_full_rebuild(&nodes, &deps).expect("well formed");

        let flattened: Vec<ObjectId> = plan.levels().iter().flatten().copied().collect();
        assert_eq!(flattened.len(), plan.len());
        assert_eq!(
            flattened.iter().copied().collect::<BTreeSet<_>>(),
            plan.order().iter().copied().collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn levels_ignore_clean_dependencies_when_measuring_depth() {
        let [plane, sketch, extrude, body] = ids();
        let nodes = [plane, sketch, extrude, body];
        let deps = [
            edge(sketch, plane),
            edge(extrude, sketch),
            edge(body, extrude),
        ];

        // The plane and sketch are clean, so the extrude starts at level 0
        // rather than inheriting depth from work nobody is redoing.
        let plan = plan_rebuild(&nodes, &deps, &[extrude]).expect("well formed");
        assert_eq!(plan.levels(), &[vec![extrude], vec![body]]);
    }

    #[test]
    fn the_plan_does_not_depend_on_input_order() {
        let [a, b, c, d] = ids();
        let deps = [edge(b, a), edge(c, b), edge(d, a)];
        let reversed: Vec<Dependency> = deps.iter().rev().copied().collect();

        let one = plan_rebuild(&[a, b, c, d], &deps, &[a]).expect("well formed");
        let other = plan_rebuild(&[d, c, b, a], &reversed, &[a]).expect("well formed");

        assert_eq!(one, other);
        assert_eq!(one.levels(), other.levels());
    }

    #[test]
    fn a_partial_plan_is_a_subsequence_of_the_full_one() {
        let [a, b, c, d, e] = ids();
        let nodes = [a, b, c, d, e];
        let deps = [edge(b, a), edge(c, b), edge(d, a), edge(e, c)];

        let full = plan_full_rebuild(&nodes, &deps).expect("well formed");
        let partial = plan_rebuild(&nodes, &deps, &[b]).expect("well formed");

        let mut remaining = partial.order().iter().copied();
        let mut next = remaining.next();
        for id in full.order() {
            if next == Some(*id) {
                next = remaining.next();
            }
        }
        assert_eq!(next, None, "the partial order disagrees with the full one");
    }

    #[test]
    fn a_full_rebuild_covers_every_object() {
        let [a, b, c] = ids();
        let plan = plan_full_rebuild(&[a, b, c], &[edge(b, a), edge(c, b)]).expect("well formed");

        assert_eq!(plan.order(), &[a, b, c]);
        assert_eq!(plan.levels(), &[vec![a], vec![b], vec![c]]);
    }

    #[test]
    fn changing_nothing_plans_nothing() {
        let [a, b] = ids();
        let plan = plan_rebuild(&[a, b], &[edge(b, a)], &[]).expect("well formed");

        assert!(plan.is_empty());
        assert_eq!(plan.len(), 0);
        assert!(plan.levels().is_empty());
    }

    #[test]
    fn a_cycle_is_refused_rather_than_partially_planned() {
        let [a, b, c] = ids();
        let err = plan_rebuild(&[a, b, c], &[edge(b, a), edge(c, b), edge(a, c)], &[a])
            .expect_err("a cyclic graph has no valid order");

        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Input);
        assert!(err.to_string().contains("cycle"));
    }

    #[test]
    fn a_dangling_edge_is_refused_rather_than_partially_planned() {
        let [present, absent] = ids();
        let err = plan_rebuild(&[present], &[edge(present, absent)], &[present])
            .expect_err("an edge into nothing is not a graph");

        assert_eq!(err.kind(), ferritecad_types::ErrorKind::Input);
        assert!(err.to_string().contains(&absent.to_string()));
    }

    #[test]
    fn parallel_edges_do_not_duplicate_an_object_in_the_plan() {
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

        let plan = plan_full_rebuild(&[a, b], &deps).expect("well formed");
        assert_eq!(plan.order(), &[a, b]);
        assert_eq!(plan.levels(), &[vec![a], vec![b]]);
    }

    #[test]
    fn an_isolated_object_is_planned_alone_at_the_first_level() {
        let [a, b, lonely] = ids();
        let plan = plan_full_rebuild(&[a, b, lonely], &[edge(b, a)]).expect("well formed");

        let mut first = vec![a, lonely];
        first.sort_unstable();
        assert_eq!(plan.levels()[0], first);
    }
}
