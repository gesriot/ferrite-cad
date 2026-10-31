// SPDX-License-Identifier: MIT
use ferritecad_types::{CadError, CanonicalHasher, ContentHash, ObjectId, Result};
use serde::{Deserialize, Serialize};

use crate::{EntityKind, EntityReference};

/// A deliberate selection within one predecessor's complete result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "faces",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum KeepSelection {
    All,
    ContainingFaces(Vec<EntityReference>),
}

impl KeepSelection {
    pub(crate) fn validate(&self) -> Result<()> {
        if let Self::ContainingFaces(faces) = self {
            if faces.is_empty() {
                return Err(CadError::input(
                    "KeepSolids needs at least one face reference",
                ));
            }
            for face in faces {
                crate::entity_reference::validate_kind(face, EntityKind::Face)?;
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for KeepSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => f.write_str("all solids"),
            Self::ContainingFaces(faces) => {
                write!(f, "solids containing {} face references", faces.len())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeepSolids {
    pub previous: ObjectId,
    pub selection: KeepSelection,
}

impl KeepSolids {
    /// The caller includes the predecessor's result key and kernel identity.
    pub fn cache_key(&self) -> ContentHash {
        let mut hash = CanonicalHasher::new("feature.keep_solids");
        hash.algorithm_version(1).bytes(&self.previous.to_bytes());
        match &self.selection {
            KeepSelection::All => {
                hash.field("all");
            }
            KeepSelection::ContainingFaces(faces) => {
                hash.field("containing_faces");
                for meaning in faces
                    .iter()
                    .map(|f| f.meaning)
                    .collect::<std::collections::BTreeSet<_>>()
                {
                    meaning.feed(&mut hash);
                }
            }
        }
        hash.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ObjectKind, ObjectPayload, ReferenceMeaning, Selection};
    use ferritecad_types::StableEntityId;

    #[test]
    fn stored_selection_round_trips_validates_and_keys_only_its_meanings() {
        let face = EntityReference::new(ReferenceMeaning {
            producer: ObjectId::new(),
            kind: EntityKind::Face,
            digest: ContentHash::of_bytes(b"face"),
            selection: Selection::One,
        });
        let mut keep = KeepSolids {
            previous: ObjectId::new(),
            selection: KeepSelection::ContainingFaces(vec![face]),
        };
        let key = keep.cache_key();
        let payload = ObjectPayload::KeepSolids(keep.clone());
        assert!(ObjectKind::KeepSolids.is_feature());
        assert_eq!(payload.previous_feature(), Some(keep.previous));
        assert_eq!(
            payload.dependencies(ObjectId::new())[0].role,
            crate::DependencyRole::Predecessor
        );
        assert_eq!(
            ObjectPayload::from_storage_bytes(&payload.to_storage_bytes().expect("encode"))
                .expect("decode"),
            payload
        );
        let mut same = face;
        same.id = StableEntityId::new();
        keep.selection = KeepSelection::ContainingFaces(vec![same, face]);
        assert_eq!(
            keep.cache_key(),
            key,
            "diagnostics and duplicate witnesses are not meaning"
        );
        keep.selection = KeepSelection::All;
        assert_ne!(keep.cache_key(), key);
        for selection in [
            KeepSelection::ContainingFaces(vec![]),
            KeepSelection::ContainingFaces(vec![EntityReference::new(ReferenceMeaning {
                kind: EntityKind::Edge,
                ..face.meaning
            })]),
        ] {
            assert!(
                ObjectPayload::KeepSolids(KeepSolids {
                    selection,
                    previous: keep.previous
                })
                .to_storage_bytes()
                .is_err()
            );
        }
    }
}
