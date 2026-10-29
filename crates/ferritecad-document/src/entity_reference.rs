// SPDX-License-Identifier: MIT
use ferritecad_types::{CadError, CanonicalHasher, ContentHash, ObjectId, Result, StableEntityId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Face,
    Edge,
    Vertex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Selection {
    One,
    AllDescendants,
}

/// Constant-size meaning; the diagnostic identity belongs to its consumption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReferenceMeaning {
    pub producer: ObjectId,
    pub kind: EntityKind,
    pub digest: ContentHash,
    pub selection: Selection,
}

impl Serialize for ReferenceMeaning {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        (
            self.producer,
            self.kind as u8,
            self.digest,
            self.selection as u8,
        )
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ReferenceMeaning {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let (producer, kind, digest, selection) =
            <(ObjectId, u8, ContentHash, u8)>::deserialize(deserializer)?;
        Ok(Self {
            producer,
            kind: match kind {
                0 => EntityKind::Face,
                1 => EntityKind::Edge,
                2 => EntityKind::Vertex,
                _ => return Err(de::Error::custom("unknown entity kind")),
            },
            digest,
            selection: match selection {
                0 => Selection::One,
                1 => Selection::AllDescendants,
                _ => return Err(de::Error::custom("unknown entity selection")),
            },
        })
    }
}

impl ReferenceMeaning {
    pub fn to_bytes(self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        ciborium::into_writer(&self, &mut bytes)
            .map_err(|error| CadError::input_because("encoding entity reference", error))?;
        Ok(bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut reader = std::io::Cursor::new(bytes);
        let meaning = ciborium::from_reader(&mut reader)
            .map_err(|error| CadError::input_because("decoding entity reference", error))?;
        if reader.position() != bytes.len() as u64 {
            return Err(CadError::input("trailing bytes after entity reference"));
        }
        Ok(meaning)
    }

    pub fn feed(self, hasher: &mut CanonicalHasher) {
        hasher
            .bytes(&self.producer.to_bytes())
            .bytes(&[self.kind as u8])
            .hash(&self.digest)
            .bytes(&[self.selection as u8]);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityReference {
    pub id: StableEntityId,
    pub meaning: ReferenceMeaning,
}

impl EntityReference {
    pub fn new(meaning: ReferenceMeaning) -> Self {
        Self {
            id: StableEntityId::new(),
            meaning,
        }
    }
}

pub(crate) fn validate_edges(edges: &[EntityReference]) -> Result<()> {
    if edges.is_empty() {
        return Err(CadError::input(
            "an edge feature needs at least one reference",
        ));
    }
    let mut meanings = std::collections::BTreeSet::new();
    for edge in edges {
        if edge.meaning.kind != EntityKind::Edge {
            return Err(CadError::input(format!(
                "reference {} must name an edge",
                edge.id
            )));
        }
        if !meanings.insert(edge.meaning) {
            return Err(CadError::input(format!(
                "reference {} repeats an edge meaning",
                edge.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ciborium::Value;

    fn reference() -> EntityReference {
        EntityReference::new(ReferenceMeaning {
            producer: ObjectId::new(),
            kind: EntityKind::Edge,
            digest: ContentHash::of_bytes(b"edge meaning"),
            selection: Selection::One,
        })
    }

    #[test]
    fn reference_meaning_is_exactly_the_compact_array_and_rejects_malformed_input() {
        let meaning = reference().meaning;
        let bytes = meaning.to_bytes().expect("encode");
        assert_eq!(bytes.len(), 54);
        assert_eq!(
            ReferenceMeaning::from_bytes(&bytes).expect("decode"),
            meaning
        );
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(ReferenceMeaning::from_bytes(&trailing).is_err());
        let Value::Array(fields) =
            ciborium::from_reader::<Value, _>(bytes.as_slice()).expect("array")
        else {
            unreachable!("reference array")
        };
        for (index, bad) in [
            (0, Value::Bytes(vec![0; 15])),
            (2, Value::Bytes(vec![0; 31])),
            (1, Value::Integer(3.into())),
            (3, Value::Integer(2.into())),
        ] {
            let mut changed = fields.clone();
            changed[index] = bad;
            let mut encoded = Vec::new();
            ciborium::into_writer(&Value::Array(changed), &mut encoded).expect("encode bad array");
            assert!(ReferenceMeaning::from_bytes(&encoded).is_err());
        }
        for length in [3, 5] {
            let mut changed = fields.clone();
            changed.resize(length, Value::Null);
            let mut encoded = Vec::new();
            ciborium::into_writer(&Value::Array(changed), &mut encoded).expect("encode bad arity");
            assert!(ReferenceMeaning::from_bytes(&encoded).is_err());
        }
    }

    #[test]
    fn edge_meanings_are_unique_and_diagnostic_ids_do_not_affect_cache_keys() {
        let first = reference();
        let mut second = reference();
        let mut feature = crate::Fillet {
            previous: ObjectId::new(),
            edges: vec![first, second],
            radius_mm: crate::Expression::constant(1.).expect("radius"),
        };
        let tolerance = ferritecad_types::Tolerance::default();
        let key = feature.cache_key(tolerance);
        feature.edges.reverse();
        feature.edges[0].id = StableEntityId::new();
        assert_eq!(feature.cache_key(tolerance), key);
        validate_edges(&feature.edges).expect("distinct meanings");
        second.meaning = first.meaning;
        assert!(validate_edges(&[first, second]).is_err());
        assert!(validate_edges(&[]).is_err());
        feature.edges[0].meaning.selection = Selection::AllDescendants;
        assert_ne!(feature.cache_key(tolerance), key);
    }
}
