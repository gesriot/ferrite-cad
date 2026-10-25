// SPDX-License-Identifier: MIT
//! Complete generative topology and profile-corner edge selection.
mod archive;
mod codec;
mod description;

pub use archive::{ArchivedFeature, archive_feature, restore_feature};
pub use codec::ARCHIVE_CACHE_KIND;
pub use generative::ModelNaming;

pub mod generative;
mod swept_edge;

mod derivation;

#[cfg(test)]
extern crate self as ferritecad_topology;
