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
    digest: ContentHash,
    node: Arc<Derivation>,
) -> Result<()> {
    if let Some(existing) = graph.get(&digest) {
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
pub(crate) fn intern(graph: &mut Derivations, node: Derivation) -> Result<Name> {
    let name = node.name();
    insert(graph, name.digest, Arc::new(node))?;
    Ok(name)
}
pub(crate) fn expression(
    graph: &mut Derivations,
    kind: SubShapeKind,
    tag: &'static str,
    feature: Option<ObjectId>,
    inputs: impl IntoIterator<Item = Name>,
    extra: u8,
) -> Result<Name> {
    intern(
        graph,
        Derivation::Expression {
            kind,
            tag,
            feature,
            inputs: inputs.into_iter().collect(),
            extra,
        },
    )
}
pub(crate) fn validate(graph: &Derivations) -> Result<()> {
    for (digest, node) in graph {
        let valid = match &**node {
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
                                && inputs.iter().all(|i| {
                                    matches!(
                                        graph.get(&i.digest).map(|v| &**v),
                                        Some(Derivation::Curve(..))
                                    )
                                })
                        }
                        "generated" | "modified" => {
                            feature.is_some() && *extra <= 4 && inputs.len() == 1
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
            || node.name().digest != *digest
            || node.inputs().any(|n| {
                graph
                    .get(&n.digest)
                    .is_none_or(|parent| parent.kind() != n.kind)
            })
        {
            return Err(CadError::topology(
                "invalid or incomplete naming derivation graph",
            ));
        }
    }
    Ok(())
}
