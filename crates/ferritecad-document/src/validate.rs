// SPDX-License-Identifier: MIT
//! Checks that a document is rebuildable before anything tries to rebuild it.
//!
//! Every check here answers a question a user would otherwise discover as a
//! confusing failure much later: is the graph acyclic, does each semantic
//! reference have a matching dependency edge, has a payload been corrupted on
//! disk. A wrong answer delivered confidently is worse than a refusal, so
//! anything that would make a rebuild meaningless is an error, and anything
//! this build merely cannot interpret is a warning.

use std::collections::{BTreeMap, BTreeSet};

use ferritecad_exchange::StoredScene;
use ferritecad_types::{ContentHash, ObjectId, Result};

use crate::document::Document;
use crate::graph::{DependencyRole, evaluation_order};
use crate::model::{ObjectKind, ObjectPayload, SolidOperation};
use crate::schema::FORMAT_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The document cannot be rebuilt as it stands.
    Error,
    /// The document is usable, but something was not fully understood.
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable machine-readable identifier, safe to match on in tests and tools.
    pub code: &'static str,
    pub message: String,
    pub object: Option<ObjectId>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ValidationReport {
    pub diagnostics: Vec<Diagnostic>,
}

impl ValidationReport {
    /// True when nothing blocks a rebuild. Warnings do not block.
    pub fn is_ok(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
    }

    fn error(&mut self, code: &'static str, object: Option<ObjectId>, message: String) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            code,
            message,
            object,
        });
    }

    fn warn(&mut self, code: &'static str, object: Option<ObjectId>, message: String) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code,
            message,
            object,
        });
    }
}

pub(crate) fn validate(document: &Document) -> Result<ValidationReport> {
    let mut report = ValidationReport::default();
    let meta = document.meta();

    if meta.format_version > FORMAT_VERSION {
        report.error(
            "meta.format-too-new",
            None,
            format!(
                "document claims format v{}, this build writes v{FORMAT_VERSION}",
                meta.format_version
            ),
        );
    }

    let objects = document.objects()?;
    check_occurrence_ownership(&objects, &mut report);
    let by_id: BTreeMap<ObjectId, &crate::document::ObjectRecord> =
        objects.iter().map(|o| (o.id, o)).collect();

    let mut edges: BTreeSet<(ObjectId, ObjectId, DependencyRole)> = BTreeSet::new();
    for dep in document.dependencies()? {
        edges.insert((dep.dependent, dep.dependency, dep.role));
    }

    for object in &objects {
        check_payload_integrity(object, &mut report);
        check_parent(object, &by_id, &mut report);
        check_semantic_references(object, &by_id, &edges, &mut report);
        if matches!(object.payload, ObjectPayload::ImportedStep(_))
            && let Err(error) = document.step_import(object.id)
        {
            report.error(
                "imported-source.invalid",
                Some(object.id),
                error.to_string(),
            );
        }
    }
    if let Err(error) = document.require_imported_source_reachability() {
        report.error("imported-source.unreachable", None, error.to_string());
    }
    check_parent_cycles(&objects, &mut report);
    check_feature_history(&objects, &mut report);

    // Ordering subsumes the cycle and dangling-edge checks, and reports which
    // objects are involved rather than merely that something is wrong.
    let nodes: Vec<ObjectId> = objects.iter().map(|o| o.id).collect();
    if let Err(error) = evaluation_order(&nodes, &document.dependencies()?) {
        report.error("graph.not-orderable", None, error.to_string());
    }

    for reference in document.topology_refs()? {
        if !by_id.contains_key(&reference.owner) {
            report.error(
                "topology-ref.orphan-owner",
                Some(reference.owner),
                format!(
                    "topology reference {} is owned by {}, which does not exist",
                    reference.id, reference.owner
                ),
            );
        }
        if !by_id.contains_key(&reference.producer_feature) {
            report.error(
                "topology-ref.missing-producer",
                Some(reference.producer_feature),
                format!(
                    "topology reference {} names {} as its producer, which does not exist",
                    reference.id, reference.producer_feature
                ),
            );
        } else if !by_id[&reference.producer_feature]
            .payload
            .kind()
            .is_some_and(ObjectKind::is_feature)
        {
            report.error(
                "topology-ref.producer-not-feature",
                Some(reference.producer_feature),
                format!(
                    "topology reference {} names {}, which is not a feature, as its producer",
                    reference.id, reference.producer_feature
                ),
            );
        }
        if reference.owner != reference.producer_feature
            && !edges.contains(&(
                reference.owner,
                reference.producer_feature,
                DependencyRole::TopologyReference,
            ))
        {
            report.error(
                "topology-ref.missing-edge",
                Some(reference.owner),
                format!(
                    "topology reference {} is owned by {} but has no topology_reference dependency on {}",
                    reference.id, reference.owner, reference.producer_feature
                ),
            );
        }
    }

    Ok(report)
}

/// Payload validation checks uniqueness inside each import. The document must
/// also reject an ID copied between otherwise valid imports, before any native
/// feature is rebuilt or any stored STEP is handed to an importer. Use the
/// objects already decoded for validation; no second document or source read.
fn check_occurrence_ownership(
    objects: &[crate::document::ObjectRecord],
    report: &mut ValidationReport,
) {
    let mut owners = BTreeMap::new();
    for object in objects {
        let ObjectPayload::ImportedStep(imported) = &object.payload else {
            continue;
        };
        let StoredScene::V3(scene) = &imported.scene else {
            continue;
        };
        for instance in &scene.instances {
            if let Some(owner) = owners.insert(instance.occurrence, object.id)
                && owner != object.id
            {
                report.error(
                    "occurrence.duplicate",
                    Some(object.id),
                    format!(
                        "occurrence {} is claimed by imported objects {owner} and {}; \
                         one placement identity must belong to one object",
                        instance.occurrence, object.id
                    ),
                );
            }
        }
    }
}

fn check_payload_integrity(object: &crate::document::ObjectRecord, report: &mut ValidationReport) {
    // The stored hash covers the raw envelope, not a re-encoding of a known
    // payload. A different producer may use another valid CBOR map order, and
    // that must not be mistaken for disk corruption.
    if ContentHash::of_bytes(object.storage_bytes()) != object.payload_hash {
        report.error(
            "object.payload-hash-mismatch",
            Some(object.id),
            format!(
                "object {} does not match its stored hash; the file may be corrupt",
                object.id
            ),
        );
    }

    if let ObjectPayload::Unknown(unknown) = &object.payload {
        report.warn(
            "object.unknown-type",
            Some(object.id),
            format!(
                "object {} has type {:?} at schema v{}, which this build preserves but cannot \
                 interpret",
                object.id, unknown.type_name, unknown.schema_version
            ),
        );
    }
}

fn check_parent(
    object: &crate::document::ObjectRecord,
    by_id: &BTreeMap<ObjectId, &crate::document::ObjectRecord>,
    report: &mut ValidationReport,
) {
    let Some(parent) = object.parent else {
        return;
    };

    if parent == object.id {
        report.error(
            "object.self-parent",
            Some(object.id),
            format!("object {} is its own parent", object.id),
        );
    } else if !by_id.contains_key(&parent) {
        report.error(
            "object.missing-parent",
            Some(object.id),
            format!("object {} names missing parent {parent}", object.id),
        );
    }
}

/// Confirms that what a payload refers to also exists as a dependency edge.
///
/// The payload says what the model means; the edge says what order to rebuild
/// in. If they disagree, a feature can be evaluated before the thing it reads,
/// which is exactly the class of bug that produces a plausible but wrong solid.
fn check_semantic_references(
    object: &crate::document::ObjectRecord,
    by_id: &BTreeMap<ObjectId, &crate::document::ObjectRecord>,
    edges: &BTreeSet<(ObjectId, ObjectId, DependencyRole)>,
    report: &mut ValidationReport,
) {
    let require = |target: ObjectId,
                   role: DependencyRole,
                   expected: ObjectKind,
                   what: &str,
                   report: &mut ValidationReport| {
        match by_id.get(&target) {
            None => report.error(
                "reference.missing-target",
                Some(object.id),
                format!("{} {} refers to missing object {target}", what, object.id),
            ),
            Some(found) if found.payload.kind() != Some(expected) => report.error(
                "reference.wrong-kind",
                Some(object.id),
                format!(
                    "{} {} expects {} to be a {}, found {}",
                    what,
                    object.id,
                    target,
                    expected.as_str(),
                    found.payload.type_name()
                ),
            ),
            Some(_) => {}
        }

        if !edges.contains(&(object.id, target, role)) {
            report.error(
                "reference.missing-edge",
                Some(object.id),
                format!(
                    "{} {} refers to {target} but no {} dependency records it",
                    what,
                    object.id,
                    role.as_str()
                ),
            );
        }
    };

    match &object.payload {
        ObjectPayload::Sketch(sketch) => {
            require(
                sketch.plane,
                DependencyRole::Plane,
                ObjectKind::DatumPlane,
                "sketch",
                report,
            );
        }
        ObjectPayload::Extrude(extrude) => {
            require(
                extrude.profile,
                DependencyRole::Profile,
                ObjectKind::Sketch,
                "extrude",
                report,
            );
            if let Some(body) = extrude.target_body {
                require(
                    body,
                    DependencyRole::TargetBody,
                    ObjectKind::Body,
                    "extrude",
                    report,
                );
            }
            // What a feature consumes and which body owns it are two different
            // facts, and only the first belongs to the feature. Saying both
            // would be two answers to one question, and the second of them
            // is what makes the graph impossible to order.
            if extrude.previous.is_some() && extrude.target_body.is_some() {
                report.error(
                    "feature.two-inputs",
                    Some(object.id),
                    format!(
                        "feature {} names both a predecessor and a target body; the body it                          belongs to is the body's own statement",
                        object.id
                    ),
                );
            }
            match (extrude.operation, extrude.previous) {
                (SolidOperation::NewBody, Some(previous)) => report.error(
                    "feature.unused-predecessor",
                    Some(object.id),
                    format!(
                        "feature {} starts a body and also names {previous} as the result it                          modifies",
                        object.id
                    ),
                ),
                (SolidOperation::NewBody, None) => {}
                (_, None) => report.error(
                    "feature.missing-predecessor",
                    Some(object.id),
                    format!(
                        "feature {} modifies an existing solid but names no earlier feature",
                        object.id
                    ),
                ),
                (_, Some(previous)) if previous == object.id => report.error(
                    "feature.self-predecessor",
                    Some(object.id),
                    format!("feature {} names itself as the result it modifies", object.id),
                ),
                (_, Some(previous)) => {
                    match by_id.get(&previous) {
                        None => report.error(
                            "reference.missing-target",
                            Some(object.id),
                            format!(
                                "feature {} modifies missing feature {previous}",
                                object.id
                            ),
                        ),
                        Some(found)
                            if !found.payload.kind().is_some_and(ObjectKind::is_feature) =>
                        {
                            report.error(
                                "reference.wrong-kind",
                                Some(object.id),
                                format!(
                                    "feature {} expects {previous} to be a feature, found {}",
                                    object.id,
                                    found.payload.type_name()
                                ),
                            );
                        }
                        Some(_) => {}
                    }
                    if !edges.contains(&(object.id, previous, DependencyRole::Predecessor)) {
                        report.error(
                            "reference.missing-edge",
                            Some(object.id),
                            format!(
                                "feature {} modifies {previous} but no predecessor dependency                                  records it",
                                object.id
                            ),
                        );
                    }
                }
            }
        }
        ObjectPayload::Revolve(revolve) => {
            require(
                revolve.profile,
                DependencyRole::Profile,
                ObjectKind::Sketch,
                "revolve",
                report,
            );
        }
        ObjectPayload::Body(body) => {
            if let Some(tip) = body.tip_feature {
                match by_id.get(&tip) {
                    None => report.error(
                        "reference.missing-target",
                        Some(object.id),
                        format!("body {} refers to missing tip feature {tip}", object.id),
                    ),
                    Some(found) if !found.payload.kind().is_some_and(ObjectKind::is_feature) => {
                        report.error(
                            "reference.wrong-kind",
                            Some(object.id),
                            format!(
                                "body {} expects {tip} to be a feature, found {}",
                                object.id,
                                found.payload.type_name()
                            ),
                        );
                    }
                    Some(_) => {}
                }
                if !edges.contains(&(object.id, tip, DependencyRole::BodyTip)) {
                    report.error(
                        "reference.missing-edge",
                        Some(object.id),
                        format!(
                            "body {} refers to tip feature {tip} but no body_tip dependency records it",
                            object.id
                        ),
                    );
                }
            }
        }
        _ => {}
    }
}

/// A feature history is a chain, and every body has at most one of them.
///
/// Two features that both modify the same earlier result would be two versions
/// of one body, and nothing in this build says which of them the body is. Two
/// bodies naming one tip would be one solid shown twice. Both are refused here
/// rather than at the evaluator, because they are facts about what the document
/// says and not about what a kernel would do with it.
fn check_feature_history(objects: &[crate::document::ObjectRecord], report: &mut ValidationReport) {
    let mut consumers: BTreeMap<ObjectId, Vec<ObjectId>> = BTreeMap::new();
    let mut owners: BTreeMap<ObjectId, Vec<ObjectId>> = BTreeMap::new();
    for object in objects {
        match &object.payload {
            ObjectPayload::Extrude(extrude) => {
                if let Some(previous) = extrude.previous {
                    consumers.entry(previous).or_default().push(object.id);
                }
            }
            ObjectPayload::Body(body) => {
                if let Some(tip) = body.tip_feature {
                    owners.entry(tip).or_default().push(object.id);
                }
            }
            _ => {}
        }
    }
    for (feature, mut claimants) in consumers {
        if claimants.len() > 1 {
            claimants.sort_unstable();
            report.error(
                "feature.forked-history",
                Some(feature),
                format!(
                    "features {} all modify {feature}, and a body has one history rather than                      several",
                    claimants
                        .iter()
                        .map(ObjectId::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
    }
    for (feature, mut bodies) in owners {
        if bodies.len() > 1 {
            bodies.sort_unstable();
            report.error(
                "body.shared-tip",
                Some(feature),
                format!(
                    "bodies {} all expose {feature} as their tip, so one solid would appear as                      several",
                    bodies
                        .iter()
                        .map(ObjectId::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
    }

    // Different tips can still own the same history. Stop at an already
    // visited feature, both to avoid traversing a shared suffix repeatedly and
    // to leave malformed cycles to the existing graph validator.
    let predecessors: BTreeMap<_, _> = objects
        .iter()
        .filter_map(|object| {
            if let ObjectPayload::Extrude(extrude) = &object.payload {
                Some((object.id, extrude.previous))
            } else {
                None
            }
        })
        .collect();
    let mut history_owners = BTreeMap::new();
    for object in objects {
        let ObjectPayload::Body(body) = &object.payload else {
            continue;
        };
        let mut current = body.tip_feature;
        while let Some(feature) = current {
            if let Some(other) = history_owners.get(&feature) {
                if *other != object.id {
                    report.error(
                        "body.shared-history",
                        Some(feature),
                        format!(
                            "bodies {other} and {} both own feature {feature} through their tips",
                            object.id,
                        ),
                    );
                }
                break;
            }
            history_owners.insert(feature, object.id);
            current = predecessors.get(&feature).copied().flatten();
        }
    }
}

/// The presentation hierarchy is distinct from the feature DAG, but it must
/// still be a forest. A two-node parent loop makes tree UI traversal recurse
/// forever even though the evaluator graph itself is acyclic.
fn check_parent_cycles(objects: &[crate::document::ObjectRecord], report: &mut ValidationReport) {
    let parents: BTreeMap<ObjectId, Option<ObjectId>> = objects
        .iter()
        .map(|object| (object.id, object.parent))
        .collect();

    for object in objects {
        let mut seen = BTreeSet::new();
        let mut current = Some(object.id);
        while let Some(id) = current {
            if !seen.insert(id) {
                report.error(
                    "object.parent-cycle",
                    Some(object.id),
                    format!("object hierarchy contains a parent cycle through {id}"),
                );
                break;
            }
            current = parents.get(&id).copied().flatten();
        }
    }
}
