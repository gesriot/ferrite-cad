// SPDX-License-Identifier: MIT
use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::{Uuid, Variant, Version};

use crate::error::CadError;

/// Defines an identifier newtype over [`Uuid`].
///
/// All FerriteCAD identifiers are UUIDv7 so that insertion order is recoverable
/// from the identifier itself, and all of them serialise to exactly sixteen
/// bytes in binary formats. The macro exists because the identifier types are
/// identical apart from the name; the distinct types are what stop an object
/// identifier being passed where a document identifier is meant.
macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a fresh time-ordered identifier.
            ///
            /// Deliberately not `Default`: `default()` reads as "an empty one"
            /// but would mint a brand new identity, which is exactly the slip
            /// that produces two objects believing they are the same one.
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Validates and wraps a UUIDv7 identifier.
            ///
            /// The timestamp order is part of the on-disk format, not a
            /// cosmetic preference. Accepting a v4 UUID through deserialisation
            /// would silently break deterministic ordering after a reload.
            pub fn try_from_uuid(uuid: Uuid) -> Result<Self, CadError> {
                if uuid.get_variant() != Variant::RFC4122
                    || uuid.get_version() != Some(Version::SortRand)
                {
                    return Err(CadError::input(format!(
                        "{} must be an RFC 4122 UUIDv7, found {}",
                        stringify!($name),
                        uuid
                    )));
                }
                Ok(Self(uuid))
            }

            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// The canonical sixteen-byte big-endian form used in storage.
            pub const fn to_bytes(self) -> [u8; 16] {
                *self.0.as_bytes()
            }

            pub fn from_bytes(bytes: [u8; 16]) -> Result<Self, CadError> {
                Self::try_from_uuid(Uuid::from_bytes(bytes))
            }

            /// Reads the canonical form from a storage blob.
            pub fn from_slice(bytes: &[u8]) -> Result<Self, CadError> {
                let bytes: [u8; 16] = bytes.try_into().map_err(|_| {
                    CadError::input(format!(
                        concat!(stringify!($name), " must be 16 bytes, found {}"),
                        bytes.len()
                    ))
                })?;
                Self::from_bytes(bytes)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = CadError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let uuid = Uuid::parse_str(s).map_err(|e| {
                    CadError::input_because(format!("malformed {}", stringify!($name)), e)
                })?;
                Self::try_from_uuid(uuid)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                if serializer.is_human_readable() {
                    serializer.serialize_str(&self.0.to_string())
                } else {
                    serializer.serialize_bytes(self.0.as_bytes())
                }
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct IdVisitor;

                impl<'v> Visitor<'v> for IdVisitor {
                    type Value = $name;

                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        write!(f, "a 16-byte UUID or its textual form")
                    }

                    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                        $name::from_slice(v).map_err(de::Error::custom)
                    }

                    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                        v.parse().map_err(de::Error::custom)
                    }
                }

                if deserializer.is_human_readable() {
                    deserializer.deserialize_str(IdVisitor)
                } else {
                    deserializer.deserialize_bytes(IdVisitor)
                }
            }
        }
    };
}

define_id! {
    /// Identifies a document. Written into `meta` and into every cache sidecar
    /// so a sidecar from a different document is rejected rather than used.
    DocumentId
}

define_id! {
    /// Identifies any persisted object: a sketch, body, feature or parameter.
    ObjectId
}

define_id! {
    /// Identifies a feature specifically.
    ///
    /// Every feature is an object, so this converts into [`ObjectId`]; the
    /// reverse conversion does not exist, because not every object is a
    /// feature.
    FeatureId
}

define_id! {
    /// Identifies the exact bytes of a file imported into a document.
    ///
    /// A source is immutable: importing different bytes mints a new identifier
    /// rather than changing what an existing one names, so an object that
    /// recorded what it was built from keeps naming those bytes and no others.
    ImportedSourceId
}

define_id! {
    /// Identifies a semantically named piece of resulting geometry — the
    /// durable half of a topology reference.
    ///
    /// The mapping from this identifier to a concrete kernel sub-shape is
    /// cache, never source of truth.
    StableEntityId
}

impl From<FeatureId> for ObjectId {
    fn from(id: FeatureId) -> Self {
        Self(*id.as_uuid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_canonical_bytes() {
        let id = ObjectId::new();
        assert_eq!(ObjectId::from_bytes(id.to_bytes()).expect("UUIDv7"), id);
        assert_eq!(
            ObjectId::from_slice(&id.to_bytes()).expect("16 bytes is always valid"),
            id
        );
    }

    #[test]
    fn rejects_wrong_length_blob() {
        let err = ObjectId::from_slice(&[0u8; 15]).expect_err("15 bytes is not a UUID");
        assert!(err.to_string().contains("16 bytes"));
    }

    #[test]
    fn round_trips_through_text() {
        let id = FeatureId::new();
        assert_eq!(id.to_string().parse::<FeatureId>().expect("valid"), id);
        assert!("not-a-uuid".parse::<FeatureId>().is_err());
    }

    #[test]
    fn rejects_a_uuid_that_is_not_v7() {
        let v4 =
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("literal UUID is valid");
        let err = ObjectId::try_from_uuid(v4).expect_err("v4 has no sortable v7 timestamp");
        assert!(err.to_string().contains("UUIDv7"));
        assert!(v4.to_string().parse::<ObjectId>().is_err());
    }

    #[test]
    fn v7_identifiers_are_time_ordered() {
        let first = ObjectId::new();
        let second = ObjectId::new();
        assert!(first.to_bytes() <= second.to_bytes());
    }

    #[test]
    fn feature_id_widens_to_object_id() {
        let feature = FeatureId::new();
        assert_eq!(ObjectId::from(feature).to_bytes(), feature.to_bytes());
    }
}
