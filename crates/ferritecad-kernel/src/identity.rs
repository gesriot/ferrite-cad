// SPDX-License-Identifier: MIT
use std::fmt;

use ferritecad_types::{CadError, CanonicalHasher, Result};

/// Which kernel, at which version, produced a result.
///
/// This is part of every cache key rather than a label. Two builds of the same
/// kernel can tessellate the same solid into different triangles and fillet the
/// same edge into a slightly different face; reusing one's output under the
/// other's key would be a wrong answer served quickly, which is the worst thing
/// a cache can do.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KernelIdentity {
    id: String,
    version: String,
    build: String,
}

impl KernelIdentity {
    /// Names a kernel.
    ///
    /// `id` distinguishes implementations (`"occt"`, `"mock"`), `version` the
    /// release (`"8.0.1"`), and `build` anything else that changes results —
    /// a patch level, a compiler, a feature switch. `build` may be empty when
    /// there is genuinely nothing to add.
    pub fn new(
        id: impl Into<String>,
        version: impl Into<String>,
        build: impl Into<String>,
    ) -> Result<Self> {
        let id = id.into();
        let version = version.into();

        if id.trim().is_empty() {
            return Err(CadError::input("kernel identity needs a non-empty id"));
        }
        if version.trim().is_empty() {
            return Err(CadError::input(format!(
                "kernel {id} must state a version; an unversioned kernel cannot key a cache"
            )));
        }

        Ok(Self {
            id,
            version,
            build: build.into(),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn build(&self) -> &str {
        &self.build
    }

    /// Feeds the identity into a cache key.
    pub fn feed(&self, hasher: &mut CanonicalHasher) {
        hasher.field("kernel.id").str(&self.id);
        hasher.field("kernel.version").str(&self.version);
        hasher.field("kernel.build").str(&self.build);
    }
}

impl fmt::Display for KernelIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.build.is_empty() {
            write!(f, "{} {}", self.id, self.version)
        } else {
            write!(f, "{} {} ({})", self.id, self.version, self.build)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(identity: &KernelIdentity) -> ferritecad_types::ContentHash {
        let mut hasher = CanonicalHasher::new("test");
        identity.feed(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn an_identity_without_a_version_is_refused() {
        assert!(KernelIdentity::new("occt", "", "").is_err());
        assert!(KernelIdentity::new("", "8.0.1", "").is_err());
        assert!(KernelIdentity::new("occt", "   ", "").is_err());
    }

    #[test]
    fn version_participates_in_the_cache_key() {
        let older = KernelIdentity::new("occt", "8.0.0", "").expect("valid");
        let newer = KernelIdentity::new("occt", "8.0.1", "").expect("valid");
        assert_ne!(key(&older), key(&newer));
    }

    #[test]
    fn implementation_participates_in_the_cache_key() {
        let real = KernelIdentity::new("occt", "8.0.1", "").expect("valid");
        let fake = KernelIdentity::new("mock", "8.0.1", "").expect("valid");
        assert_ne!(key(&real), key(&fake));
    }

    #[test]
    fn build_participates_in_the_cache_key() {
        let one = KernelIdentity::new("occt", "8.0.1", "gcc-13").expect("valid");
        let other = KernelIdentity::new("occt", "8.0.1", "msvc-19").expect("valid");
        assert_ne!(key(&one), key(&other));
    }

    #[test]
    fn the_same_identity_keys_the_same_way() {
        let one = KernelIdentity::new("occt", "8.0.1", "gcc-13").expect("valid");
        let other = KernelIdentity::new("occt", "8.0.1", "gcc-13").expect("valid");
        assert_eq!(key(&one), key(&other));
    }

    #[test]
    fn field_boundaries_do_not_smear() {
        // "occt" + "8.0.1" must not key the same as "occt8" + ".0.1".
        let one = KernelIdentity::new("occt", "8.0.1", "").expect("valid");
        let other = KernelIdentity::new("occt8", ".0.1", "").expect("valid");
        assert_ne!(key(&one), key(&other));
    }
}
