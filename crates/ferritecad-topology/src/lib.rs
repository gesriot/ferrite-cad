// SPDX-License-Identifier: MIT
//! Complete generative topology and temporary document-role translation.
mod archive;
mod codec;
mod map;
mod resolve;

pub use archive::{ArchivedFeature, archive_feature, restore_feature};
pub use codec::ARCHIVE_CACHE_KIND;
pub use map::TopologyMap;
pub use resolve::resolve;

pub mod generative;
mod generative_legacy;

pub mod derivation;

#[cfg(test)]
extern crate self as ferritecad_topology;
