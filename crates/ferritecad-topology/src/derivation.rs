// SPDX-License-Identifier: MIT
use crate::generative::{Name, cap_root, curve_root, hash_name};
use ferritecad_kernel::SubShapeKind;
use ferritecad_types::{CadError, ContentHash, ObjectId, Result, StableEntityId};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Derivation {
    Curve(ObjectId, StableEntityId, Option<ObjectId>),
    Cap(ObjectId, bool, Option<ObjectId>),
    Expression {
        kind: SubShapeKind,
        tag: &'static str,
        feature: Option<ObjectId>,
        inputs: BTreeSet<Name>,
        extra: u8,
    },
}
pub type Derivations = BTreeMap<ContentHash, Arc<Derivation>>;

impl Derivation {
    pub(crate) fn requires_ancestry(&self) -> bool {
        matches!(
            self,
            Self::Expression {
                tag: "generated" | "boundary" | "split-boundary-history",
                ..
            }
        )
    }
    pub fn name(&self) -> Name {
        match self {
            Self::Curve(feature, curve, plane) => curve_root(*feature, *curve, *plane),
            Self::Cap(feature, end, plane) => cap_root(*feature, *end, *plane),
            Self::Expression {
                kind,
                tag,
                feature,
                inputs,
                extra,
            } => hash_name(*kind, tag, *feature, inputs.iter().copied(), *extra),
        }
    }
    pub(crate) fn kind(&self) -> SubShapeKind {
        match self {
            Self::Curve(..) => SubShapeKind::Edge,
            Self::Cap(..) => SubShapeKind::Face,
            Self::Expression { kind, .. } => *kind,
        }
    }
    pub(crate) fn inputs(&self) -> impl Iterator<Item = Name> + '_ {
        match self {
            Self::Expression { inputs, .. } => Some(inputs),
            _ => None,
        }
        .into_iter()
        .flatten()
        .copied()
    }
}

pub(crate) fn insert(
    graph: &mut Derivations,
    inherited: &Derivations,
    digest: ContentHash,
    node: Arc<Derivation>,
) -> Result<()> {
    if let Some(existing) = inherited.get(&digest).or_else(|| graph.get(&digest)) {
        if **existing != *node {
            return Err(CadError::topology(format!(
                "unequal derivations collide at {digest}; refusing to merge names"
            )));
        }
    } else {
        graph.insert(digest, node);
    }
    Ok(())
}
pub(crate) fn intern(
    graph: &mut Derivations,
    inherited: &Derivations,
    node: Derivation,
) -> Result<Name> {
    let name = node.name();
    insert(graph, inherited, name.digest, Arc::new(node))?;
    Ok(name)
}
pub(crate) fn expression(
    graph: &mut Derivations,
    inherited: &Derivations,
    kind: SubShapeKind,
    tag: &'static str,
    feature: Option<ObjectId>,
    inputs: impl IntoIterator<Item = Name>,
    extra: u8,
) -> Result<Name> {
    intern(
        graph,
        inherited,
        Derivation::Expression {
            kind,
            tag,
            feature,
            inputs: inputs.into_iter().collect(),
            extra,
        },
    )
}
// Constructors check local nodes once. External operands are checked against the model.
pub(crate) fn validate(graph: &Derivations) -> Result<()> {
    for (digest, node) in graph {
        validate_node(*digest, node, graph)?;
    }
    Ok(())
}

pub(crate) fn validate_node(
    digest: ContentHash,
    node: &Derivation,
    graph: &Derivations,
) -> Result<()> {
    let valid = match node {
        Derivation::Curve(..) | Derivation::Cap(..) => true,
        Derivation::Expression {
            kind,
            tag,
            feature,
            inputs,
            extra,
        } => {
            let elementary = matches!(
                kind,
                SubShapeKind::Face | SubShapeKind::Edge | SubShapeKind::Vertex
            );
            elementary
                && match *tag {
                    "curve-end" => {
                        *kind == SubShapeKind::Vertex
                            && feature.is_none()
                            && *extra <= 1
                            && inputs.len() == 1
                    }
                    "generated" | "modified" => {
                        feature.is_some()
                            && ferritecad_kernel::NamingOperation::from_tag(*extra).is_some()
                            && inputs.len() == 1
                    }
                    "merge" | "split-boundary-history" => {
                        *kind == SubShapeKind::Face
                            && feature.is_some()
                            && *extra == 0
                            && !inputs.is_empty()
                    }
                    "incident-faces" => {
                        *kind != SubShapeKind::Face
                            && feature.is_none()
                            && *extra == 0
                            && !inputs.is_empty()
                            && inputs.iter().all(|i| i.kind == SubShapeKind::Face)
                    }
                    "boundary" => {
                        *kind != SubShapeKind::Face
                            && feature.is_some()
                            && *extra == 0
                            && !inputs.is_empty()
                    }
                    _ => false,
                }
        }
    };
    if !valid
        || node.name().digest != digest
        || node.inputs().any(|input| {
            graph
                .get(&input.digest)
                .is_some_and(|parent| validate_operand(node, input, parent).is_err())
        })
    {
        return Err(CadError::topology(
            "invalid or incomplete naming derivation graph",
        ));
    }
    Ok(())
}

// Closure and operand kinds depend on the prepared model, not on recomputing a digest.
pub(crate) fn validate_operand(node: &Derivation, input: Name, parent: &Derivation) -> Result<()> {
    if parent.kind() != input.kind
        || (matches!(
            node,
            Derivation::Expression {
                tag: "curve-end",
                ..
            }
        ) && !matches!(parent, Derivation::Curve(..)))
    {
        return Err(CadError::topology(
            "invalid or incomplete naming derivation graph",
        ));
    }
    Ok(())
}
