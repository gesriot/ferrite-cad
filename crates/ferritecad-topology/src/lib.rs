// SPDX-License-Identifier: MIT
//! Complete generative topology and consumed entity references.
mod archive;
mod codec;
mod description;

pub use archive::{ArchivedFeature, archive_feature, restore_feature};
pub use codec::ARCHIVE_CACHE_KIND;
pub use generative::ModelNaming;

pub mod generative;

mod derivation;

#[cfg(test)]
extern crate self as ferritecad_topology;
