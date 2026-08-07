use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::CadError;

/// Rejects values that cannot be hashed reproducibly and collapses `-0.0`.
///
/// `NaN` and the infinities are not valid model data. Accepting them here would
/// mean a document whose hash depends on which bit pattern a platform produced,
/// so they are refused at the boundary instead.
pub fn normalize_f64(value: f64) -> Result<f64, CadError> {
    if !value.is_finite() {
        return Err(CadError::input(format!(
            "model values must be finite, found {value}"
        )));
    }
    // -0.0 == 0.0 compares true but has a different bit pattern, and therefore
    // a different hash. Only the positive zero is stored.
    Ok(if value == 0.0 { 0.0 } else { value })
}

/// A BLAKE3-256 digest of canonically serialised inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, CadError> {
        let bytes: [u8; 32] = bytes.try_into().map_err(|_| {
            CadError::input(format!(
                "content hash must be 32 bytes, found {}",
                bytes.len()
            ))
        })?;
        Ok(Self(bytes))
    }

    /// Hashes a byte string directly. Used for content-addressed cache blobs,
    /// whose bytes are already in their final form.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for ContentHash {
    type Err = CadError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 {
            return Err(CadError::input(format!(
                "content hash must be 64 hex characters, found {}",
                s.len()
            )));
        }
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|e| CadError::input_because("malformed content hash", e))?;
        }
        Ok(Self(bytes))
    }
}

/// Builds a [`ContentHash`] from typed, length-prefixed fields.
///
/// Every method writes a domain tag before the value, so a struct of two
/// strings cannot collide with a struct of one longer string, and an integer
/// cannot collide with the bytes that spell it. Callers must feed an algorithm
/// version and the tolerances in use before the inputs themselves; a cache hit
/// across two different algorithm versions would be a wrong result, not a
/// stale one.
#[derive(Debug, Clone)]
pub struct CanonicalHasher {
    inner: blake3::Hasher,
}

impl CanonicalHasher {
    /// Starts a hash for `domain`, e.g. `"feature.extrude"` or `"tessellation"`.
    pub fn new(domain: &str) -> Self {
        let mut hasher = Self {
            inner: blake3::Hasher::new(),
        };
        hasher.tagged(b"domain", domain.as_bytes());
        hasher
    }

    /// The version of the algorithm producing the hashed result.
    pub fn algorithm_version(&mut self, version: u32) -> &mut Self {
        self.tagged(b"algver", &version.to_be_bytes());
        self
    }

    pub fn field(&mut self, name: &str) -> &mut Self {
        self.tagged(b"field", name.as_bytes());
        self
    }

    pub fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.tagged(b"bytes", value);
        self
    }

    pub fn str(&mut self, value: &str) -> &mut Self {
        self.tagged(b"str", value.as_bytes());
        self
    }

    pub fn u64(&mut self, value: u64) -> &mut Self {
        self.tagged(b"u64", &value.to_be_bytes());
        self
    }

    pub fn bool(&mut self, value: bool) -> &mut Self {
        self.tagged(b"bool", &[u8::from(value)]);
        self
    }

    /// Feeds a real number, refusing anything that is not finite.
    pub fn f64(&mut self, value: f64) -> Result<&mut Self, CadError> {
        let normalized = normalize_f64(value)?;
        self.tagged(b"f64", &normalized.to_be_bytes());
        Ok(self)
    }

    pub fn hash(&mut self, value: &ContentHash) -> &mut Self {
        self.tagged(b"hash", value.as_bytes());
        self
    }

    pub fn finish(&self) -> ContentHash {
        ContentHash(*self.inner.finalize().as_bytes())
    }

    fn tagged(&mut self, tag: &[u8], value: &[u8]) {
        self.inner.update(tag);
        // Length prefix, so concatenated fields cannot be re-parsed as a
        // different sequence of fields.
        self.inner.update(&(value.len() as u64).to_be_bytes());
        self.inner.update(value);
    }
}

impl Serialize for ContentHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_string())
        } else {
            serializer.serialize_bytes(&self.0)
        }
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct HashVisitor;

        impl<'v> Visitor<'v> for HashVisitor {
            type Value = ContentHash;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a 32-byte digest or its hex form")
            }

            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                ContentHash::from_slice(v).map_err(de::Error::custom)
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                v.parse().map_err(de::Error::custom)
            }
        }

        if deserializer.is_human_readable() {
            deserializer.deserialize_str(HashVisitor)
        } else {
            deserializer.deserialize_bytes(HashVisitor)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_zero_normalizes() {
        assert_eq!(
            normalize_f64(-0.0).expect("zero is finite").to_be_bytes(),
            0.0f64.to_be_bytes()
        );
    }

    #[test]
    fn non_finite_values_are_refused() {
        assert!(normalize_f64(f64::NAN).is_err());
        assert!(normalize_f64(f64::INFINITY).is_err());
        assert!(normalize_f64(f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn signed_zeros_hash_identically() {
        let mut a = CanonicalHasher::new("test");
        a.f64(-0.0).expect("finite");
        let mut b = CanonicalHasher::new("test");
        b.f64(0.0).expect("finite");
        assert_eq!(a.finish(), b.finish());
    }

    #[test]
    fn field_boundaries_are_unambiguous() {
        let mut a = CanonicalHasher::new("test");
        a.str("ab").str("c");
        let mut b = CanonicalHasher::new("test");
        b.str("a").str("bc");
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn domain_separates_identical_inputs() {
        let mut a = CanonicalHasher::new("feature.extrude");
        a.f64(10.0).expect("finite");
        let mut b = CanonicalHasher::new("feature.revolve");
        b.f64(10.0).expect("finite");
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn algorithm_version_changes_the_key() {
        let mut a = CanonicalHasher::new("feature.extrude");
        a.algorithm_version(1).f64(10.0).expect("finite");
        let mut b = CanonicalHasher::new("feature.extrude");
        b.algorithm_version(2).f64(10.0).expect("finite");
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn hash_round_trips_through_hex() {
        let hash = ContentHash::of_bytes(b"profile");
        assert_eq!(
            hash.to_string().parse::<ContentHash>().expect("valid hex"),
            hash
        );
    }
}
